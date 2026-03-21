//! BEP 19: WebSeed HTTP piece fetching.
//!
//! Downloads torrent pieces from HTTP/HTTPS URLs listed in the torrent's
//! `url-list` field.  Each web seed URL is treated as a virtual peer that
//! always has all pieces.  Pieces are fetched with HTTP Range requests,
//! verified against the torrent's SHA-1 piece hashes, and fed into the
//! normal piece-completion pipeline.

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use librtbit_core::lengths::ValidPieceIndex;
use reqwest::header;
use tracing::{debug, info, trace, warn};

use super::TorrentStateLive;

/// Configuration constants for web seed behaviour.
const WEBSEED_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Minimum delay between successive requests to the same web seed URL to
/// avoid hammering the server.
const WEBSEED_MIN_INTERVAL: Duration = Duration::from_millis(500);
/// Back-off duration on transient HTTP errors (5xx, timeout).
const WEBSEED_ERROR_BACKOFF: Duration = Duration::from_secs(30);
/// Back-off on 429 Too Many Requests.
const WEBSEED_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(60);

impl TorrentStateLive {
    /// Spawn a background task for each web seed URL that fetches pieces via
    /// HTTP Range requests.
    pub(crate) fn start_webseed_tasks(self: &Arc<Self>, client: reqwest::Client) {
        // Respect the private flag: private torrents must not use web seeds.
        if self.metadata.info.info().private {
            debug!("skipping web seeds for private torrent");
            return;
        }

        let urls = &self.shared.web_seed_urls;
        if urls.is_empty() {
            return;
        }

        for url in urls.iter() {
            let state = Arc::clone(self);
            let client = client.clone();
            let url = url.clone();
            let cancel = self.cancellation_token.clone();

            let span = tracing::debug_span!(
                parent: self.shared.span.clone(),
                "webseed",
                url = %url,
            );

            librtbit_core::spawn_utils::spawn_with_cancel(
                span,
                format!("[{}]webseed:{}", self.shared.id, url),
                cancel,
                async move { state.webseed_worker(client, url).await },
            );
        }
    }

    /// The main loop for a single web seed URL.
    async fn webseed_worker(
        self: Arc<Self>,
        client: reqwest::Client,
        base_url: String,
    ) -> anyhow::Result<()> {
        info!(url = %base_url, "web seed worker started");

        loop {
            // If we are finished, stop.
            if self.is_finished() {
                debug!(url = %base_url, "torrent finished, stopping web seed worker");
                return Ok(());
            }

            // Find a piece that needs downloading and is not already in-flight.
            let piece_index = match self.pick_webseed_piece() {
                Some(p) => p,
                None => {
                    // Nothing to do right now; wait for new pieces to become available
                    // (e.g. piece hash failures causing re-queue).
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };

            let piece_length = self.lengths.piece_length(piece_index) as u64;
            let piece_offset = self.lengths.piece_offset(piece_index);

            // Build the HTTP request.
            let request_result = self
                .fetch_piece_http(&client, &base_url, piece_index, piece_offset, piece_length)
                .await;

            match request_result {
                Ok(data) => {
                    // Verify the piece hash.
                    let hash_ok = self
                        .shared
                        .spawner
                        .block_in_place(|| -> anyhow::Result<bool> {
                            self.file_ops().write_piece_from_raw(piece_index, &data)?;
                            self.file_ops().check_piece(piece_index)
                        })
                        .context("error writing/checking webseed piece")?;

                    if hash_ok {
                        trace!(piece = piece_index.get(), "webseed piece hash OK");
                        {
                            let mut g = self.lock_write("webseed_mark_piece");
                            g.get_pieces_mut()?.mark_piece_hash_ok(piece_index);
                        }

                        // Update stats.
                        self.stats.downloaded_and_checked_bytes.fetch_add(
                            piece_length,
                            std::sync::atomic::Ordering::Release,
                        );
                        self.stats.downloaded_and_checked_pieces.fetch_add(
                            1,
                            std::sync::atomic::Ordering::Release,
                        );
                        self.stats
                            .have_bytes
                            .fetch_add(piece_length, std::sync::atomic::Ordering::Relaxed);
                        self.stats
                            .fetched_bytes
                            .fetch_add(piece_length, std::sync::atomic::Ordering::Relaxed);

                        self.on_piece_completed(piece_index)?;
                        self.transmit_haves(piece_index);
                    } else {
                        warn!(
                            piece = piece_index.get(),
                            url = %base_url,
                            "webseed piece hash mismatch"
                        );
                        let mut g = self.lock_write("webseed_hash_fail");
                        g.get_pieces_mut()?.mark_piece_hash_failed(piece_index);
                    }
                }
                Err(WebSeedFetchError::NotFound) => {
                    warn!(url = %base_url, "webseed returned 404, disabling this seed");
                    return Ok(());
                }
                Err(WebSeedFetchError::RateLimited) => {
                    debug!(url = %base_url, "webseed rate-limited (429), backing off");
                    tokio::time::sleep(WEBSEED_RATE_LIMIT_BACKOFF).await;
                }
                Err(WebSeedFetchError::ServerError(status)) => {
                    debug!(url = %base_url, %status, "webseed server error, backing off");
                    tokio::time::sleep(WEBSEED_ERROR_BACKOFF).await;
                }
                Err(WebSeedFetchError::Other(e)) => {
                    debug!(url = %base_url, "webseed request error: {e:#}, backing off");
                    tokio::time::sleep(WEBSEED_ERROR_BACKOFF).await;
                }
            }

            // Rate-limit requests to this URL.
            tokio::time::sleep(WEBSEED_MIN_INTERVAL).await;
        }
    }

    /// Pick a piece that still needs downloading and is not currently in-flight
    /// from any peer.  Returns `None` if all pieces are either done or in-flight.
    fn pick_webseed_piece(&self) -> Option<ValidPieceIndex> {
        let g = self.lock_read("webseed_pick");
        let pieces = g.get_pieces().ok()?;
        let chunks = pieces.chunks();
        let have = chunks.get_have_pieces().as_slice();
        let selected = chunks.get_selected_pieces();

        for idx in 0..self.lengths.total_pieces() {
            let piece = self.lengths.validate_piece_index(idx)?;
            let i = idx as usize;
            // Skip pieces we already have.
            if have[i] {
                continue;
            }
            // Skip unselected pieces.
            if !selected[i] {
                continue;
            }
            // Skip pieces that are currently in-flight (being downloaded by a peer).
            if pieces.get_inflight(piece).is_some() {
                continue;
            }
            return Some(piece);
        }
        None
    }

    /// Fetch piece data from a web seed URL using HTTP Range requests.
    ///
    /// For single-file torrents the base URL points directly to the file.
    /// For multi-file torrents the base URL is a directory and each file is
    /// accessed as `base_url/path/to/file`.
    async fn fetch_piece_http(
        &self,
        client: &reqwest::Client,
        base_url: &str,
        _piece_index: ValidPieceIndex,
        piece_offset: u64,
        piece_length: u64,
    ) -> Result<Vec<u8>, WebSeedFetchError> {
        let is_single_file = self.metadata.info.info().length.is_some();

        if is_single_file {
            // Single-file torrent: the URL is the file itself.
            let range_end = piece_offset + piece_length - 1;
            let range_header = format!("bytes={piece_offset}-{range_end}");

            let resp = client
                .get(base_url)
                .header(header::RANGE, &range_header)
                .timeout(WEBSEED_REQUEST_TIMEOUT)
                .send()
                .await
                .map_err(|e| WebSeedFetchError::Other(e.into()))?;

            check_response_status(&resp)?;

            let bytes = resp
                .bytes()
                .await
                .map_err(|e| WebSeedFetchError::Other(e.into()))?;
            Ok(bytes.to_vec())
        } else {
            // Multi-file torrent: piece may span multiple files.
            // We need to figure out which file(s) this piece covers and
            // fetch the appropriate range from each.
            let mut result = Vec::with_capacity(piece_length as usize);
            let mut remaining = piece_length;
            let mut abs_offset = piece_offset;

            for fd in self.metadata.info.iter_file_details_ext() {
                if abs_offset >= fd.offset + fd.details.len {
                    continue;
                }
                if remaining == 0 {
                    break;
                }

                let file_start = if abs_offset > fd.offset {
                    abs_offset - fd.offset
                } else {
                    0
                };
                let available_in_file = fd.details.len - file_start;
                let to_read = std::cmp::min(remaining, available_in_file);
                let range_end = file_start + to_read - 1;
                let range_header = format!("bytes={file_start}-{range_end}");

                // Build the URL for this file.
                let file_path = fd.details.filename.to_pathbuf();
                let file_path_str = file_path
                    .components()
                    .map(|c| {
                        urlencoding::encode(
                            c.as_os_str()
                                .to_str()
                                .unwrap_or(""),
                        )
                        .into_owned()
                    })
                    .collect::<Vec<_>>()
                    .join("/");

                let url = format!(
                    "{}/{}",
                    base_url.trim_end_matches('/'),
                    file_path_str
                );

                let resp = client
                    .get(&url)
                    .header(header::RANGE, &range_header)
                    .timeout(WEBSEED_REQUEST_TIMEOUT)
                    .send()
                    .await
                    .map_err(|e| WebSeedFetchError::Other(e.into()))?;

                check_response_status(&resp)?;

                let bytes = resp
                    .bytes()
                    .await
                    .map_err(|e| WebSeedFetchError::Other(e.into()))?;
                result.extend_from_slice(&bytes);

                remaining -= to_read;
                abs_offset += to_read;
            }

            Ok(result)
        }
    }
}

enum WebSeedFetchError {
    NotFound,
    RateLimited,
    ServerError(reqwest::StatusCode),
    Other(anyhow::Error),
}

fn check_response_status(resp: &reqwest::Response) -> Result<(), WebSeedFetchError> {
    let status = resp.status();
    if status.is_success() || status == reqwest::StatusCode::PARTIAL_CONTENT {
        return Ok(());
    }
    match status {
        reqwest::StatusCode::NOT_FOUND => Err(WebSeedFetchError::NotFound),
        reqwest::StatusCode::TOO_MANY_REQUESTS => Err(WebSeedFetchError::RateLimited),
        s if s.is_server_error() => Err(WebSeedFetchError::ServerError(s)),
        s => Err(WebSeedFetchError::Other(anyhow::anyhow!(
            "unexpected HTTP status: {s}"
        ))),
    }
}
