use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use futures::{StreamExt, TryFutureExt, stream::FuturesUnordered};
use librtbit_core::hash_id::Id20;
use librqbit_utp::BindDevice;
use tokio::io::AsyncReadExt;
use tokio::sync::Semaphore;
use tracing::{Instrument, debug, debug_span, trace, warn};

/// Maximum number of incoming connection handshakes that can run concurrently.
/// This prevents FD exhaustion during connection bursts.
const MAX_CONCURRENT_HANDSHAKES: usize = 64;

use crate::{
    listen::Accept,
    mse::{self, EncryptionMode},
    read_buf::ReadBuf,
    stream_connect::ConnectionKind,
    torrent_state::TorrentStateLive,
    type_aliases::{BoxAsyncReadVectored, BoxAsyncWrite},
    vectored_traits::AsyncReadVectoredIntoCompat,
};

use super::Session;
use super::types::CheckedIncomingConnection;

impl Session {
    /// Collect all active info hashes from the session database.
    fn active_info_hashes(&self) -> Vec<Id20> {
        self.db
            .read()
            .torrents
            .values()
            .map(|t| t.info_hash())
            .collect()
    }

    pub(super) async fn check_incoming_connection(
        self: Arc<Self>,
        addr: SocketAddr,
        kind: ConnectionKind,
        mut reader: BoxAsyncReadVectored,
        writer: BoxAsyncWrite,
    ) -> anyhow::Result<(Arc<TorrentStateLive>, CheckedIncomingConnection)> {
        let rwtimeout = self
            .peer_opts
            .read_write_timeout
            .unwrap_or_else(|| Duration::from_secs(10));

        let incoming_ip = addr.ip();
        if self.blocklist.has(incoming_ip) {
            self.stats
                .counters
                .blocked_incoming
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            bail!("Incoming ip {incoming_ip} is in blocklist");
        }
        if self.allowlist.as_ref().is_some_and(|l| !l.has(incoming_ip)) {
            self.stats
                .counters
                .blocked_incoming
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            bail!("Incoming ip {incoming_ip} is not in allowlist");
        }

        let encryption_mode = self.connector.encryption;

        // Peek at the first byte to detect MSE vs plain BitTorrent.
        // Plain BT starts with 0x13 (length of "BitTorrent protocol").
        // MSE starts with random DH key bytes.
        let mut peek = [0u8; 1];
        reader.read_exact(&mut peek).await.context("reading first byte for protocol detection")?;

        let is_mse = peek[0] != 0x13;

        if is_mse && encryption_mode == EncryptionMode::Disabled {
            bail!("received MSE connection but encryption is disabled");
        }

        if !is_mse && encryption_mode == EncryptionMode::Forced {
            bail!("received plain BitTorrent connection but encryption is forced");
        }

        if is_mse {
            // MSE handshake. The first byte is part of Ya (the DH public key).
            // We need to prepend it back and run the responder handshake.
            let info_hashes = self.active_info_hashes();

            // Create a reader that prepends the peeked byte
            let prefixed_reader = mse::EncryptedReader::with_prefix(reader, None, peek.to_vec());

            let (matched_info_hash, result) =
                mse::mse_handshake_responder(prefixed_reader, writer, &info_hashes, encryption_mode)
                    .await
                    .context("MSE responder handshake failed")?;

            debug!(
                encryption = %result.encryption_status,
                info_hash = ?matched_info_hash,
                "MSE incoming handshake complete"
            );

            // Now the BT handshake happens inside the (potentially encrypted) stream
            let mut reader: BoxAsyncReadVectored = Box::new(result.reader.into_vectored_compat());
            let writer: BoxAsyncWrite = Box::new(result.writer);

            let mut read_buf = ReadBuf::new();
            let h = read_buf
                .read_handshake(&mut reader, rwtimeout)
                .await
                .context("error reading BT handshake after MSE")?;
            trace!("received BT handshake (post-MSE) from {addr}: {:?}", h);

            if h.peer_id == self.peer_id {
                bail!("seems like we are connecting to ourselves, ignoring");
            }

            // Verify the info_hash matches what was negotiated in MSE
            if h.info_hash != matched_info_hash {
                bail!(
                    "BT handshake info_hash {:?} doesn't match MSE-negotiated {:?}",
                    h.info_hash,
                    matched_info_hash
                );
            }

            let (id, torrent) = self
                .db
                .read()
                .get_by_info_hash(h.info_hash)
                .map(|(id, t)| (id, t.clone()))
                .with_context(|| format!("didn't find a matching torrent {:?}", h.info_hash))?;

            let live = torrent
                .live_wait_initializing(Duration::from_secs(5))
                .await
                .with_context(|| format!("torrent {id} is not live, ignoring connection"))?;

            Ok((
                live,
                CheckedIncomingConnection {
                    addr,
                    reader,
                    writer,
                    kind,
                    handshake: h,
                    read_buf,
                },
            ))
        } else {
            // Plain BitTorrent connection. Prepend the peeked byte back.
            let prefixed_reader = mse::EncryptedReader::with_prefix(reader, None, peek.to_vec());
            let mut reader: BoxAsyncReadVectored =
                Box::new(prefixed_reader.into_vectored_compat());

            let mut read_buf = ReadBuf::new();
            let h = read_buf
                .read_handshake(&mut reader, rwtimeout)
                .await
                .context("error reading handshake")?;
            trace!("received handshake from {addr}: {:?}", h);

            if h.peer_id == self.peer_id {
                bail!("seems like we are connecting to ourselves, ignoring");
            }

            let (id, torrent) = self
                .db
                .read()
                .get_by_info_hash(h.info_hash)
                .map(|(id, t)| (id, t.clone()))
                .with_context(|| format!("didn't find a matching torrent {:?}", h.info_hash))?;

            let live = torrent
                .live_wait_initializing(Duration::from_secs(5))
                .await
                .with_context(|| format!("torrent {id} is not live, ignoring connection"))?;

            Ok((
                live,
                CheckedIncomingConnection {
                    addr,
                    reader,
                    writer,
                    kind,
                    handshake: h,
                    read_buf,
                },
            ))
        }
    }

    pub(super) async fn task_listener<A: Accept>(self: Arc<Self>, l: A) -> anyhow::Result<()> {
        let mut futs = FuturesUnordered::new();
        let session = Arc::downgrade(&self);
        let handshake_sem = Arc::new(Semaphore::new(MAX_CONCURRENT_HANDSHAKES));
        drop(self);

        loop {
            tokio::select! {
                r = l.accept() => {
                    match r {
                        Ok((addr, (read, write))) => {
                            let permit = match handshake_sem.clone().try_acquire_owned() {
                                Ok(p) => p,
                                Err(_) => {
                                    debug!("max concurrent handshakes ({MAX_CONCURRENT_HANDSHAKES}) reached, dropping connection from {addr}");
                                    continue;
                                }
                            };
                            trace!("accepted connection from {addr}");
                            let session = session.upgrade().context("session is dead")?;
                            let span = debug_span!(parent: session.rs(), "incoming", addr=%addr);
                            futs.push(
                                async move {
                                    let result = session.check_incoming_connection(addr, A::KIND, Box::new(read), Box::new(write))
                                        .map_err(|e| {
                                            debug!("error checking incoming connection: {e:#}");
                                            e
                                        })
                                        .await;
                                    drop(permit);
                                    result
                                }
                                .instrument(span)
                            );
                        }
                        Err(e) => {
                            warn!("error accepting: {e:#}");
                            // Whatever is the reason, ensure we are not stuck trying to
                            // accept indefinitely.
                            tokio::time::sleep(Duration::from_secs(10)).await;
                            continue
                        }
                    }
                },
                Some(Ok((live, checked))) = futs.next(), if !futs.is_empty() => {
                    let (addr, kind) = (checked.addr, checked.kind);
                    if let Err(e) = live.add_incoming_peer(checked) {
                        warn!(?addr, ?kind, "error handing over incoming connection: {e:#}");
                    }
                },
            }
        }
    }

    pub(super) async fn task_upnp_port_forwarder(
        port: u16,
        bind_device: Option<BindDevice>,
    ) -> anyhow::Result<()> {
        let pf = librtbit_upnp::UpnpPortForwarder::new(vec![port], None, bind_device)?;
        pf.run_forever().await
    }
}
