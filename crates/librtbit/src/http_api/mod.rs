use anyhow::Context;
use axum::extract::{ConnectInfo, Request};
use axum::middleware::Next;
use axum::response::IntoResponse;
#[cfg(any(feature = "webui", feature = "prometheus"))]
use axum::routing::get;
use base64::Engine;
use futures::FutureExt;
use futures::future::BoxFuture;
use http::{HeaderMap, StatusCode};
use librqbit_dualstack_sockets::TcpListener;
use std::sync::Arc;
use tower_http::trace::{DefaultOnFailure, DefaultOnResponse, OnFailure};
use tracing::{Span, debug, debug_span, info};

use axum::Router;

use crate::api::Api;

use crate::ApiError;
use crate::api::Result;

pub mod auth;
mod handlers;
mod timeout;
#[cfg(feature = "webui")]
mod webui;

#[cfg(feature = "swagger")]
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        handlers::torrents::h_torrents_list,
        handlers::torrents::h_torrents_post,
        handlers::torrents::h_torrent_details,
        handlers::torrents::h_torrent_haves,
        handlers::torrents::h_torrent_stats_v0,
        handlers::torrents::h_torrent_stats_v1,
        handlers::torrents::h_peer_stats,
        handlers::torrents::h_torrent_action_pause,
        handlers::torrents::h_torrent_action_start,
        handlers::torrents::h_torrent_action_forget,
        handlers::torrents::h_torrent_action_delete,
        handlers::torrents::h_torrent_action_update_only_files,
        handlers::torrents::h_session_stats,
        handlers::torrents::h_peer_stats_prometheus,
        handlers::torrents::h_metadata,
        handlers::torrents::h_add_peers,
        handlers::torrents::h_create_torrent,
        handlers::configure::h_update_session_ratelimits,
        handlers::configure::h_get_session_ratelimits,
        handlers::dht::h_dht_stats,
        handlers::dht::h_dht_table,
        handlers::logging::h_set_rust_log,
        handlers::logging::h_stream_logs,
        handlers::streaming::h_torrent_stream_file,
        handlers::playlist::h_torrent_playlist,
        handlers::playlist::h_global_playlist,
        handlers::other::h_resolve_magnet,
    ),
    components(schemas(
        crate::api::TorrentListResponse,
        crate::api::TorrentDetailsResponse,
        crate::api::TorrentDetailsResponseFile,
        crate::api::EmptyJsonResponse,
        crate::api::ApiAddTorrentResponse,
        crate::limits::LimitsConfig,
        handlers::torrents::UpdateOnlyFilesRequest,
    ))
)]
struct ApiDoc;

/// An HTTP server for the API.
pub struct HttpApi {
    api: Api,
    opts: HttpApiOptions,
}

#[derive(Default)]
pub struct HttpApiOptions {
    pub read_only: bool,
    pub basic_auth: Option<(String, String)>,
    // Allow creating torrents via API.
    pub allow_create: bool,
    pub token_store: Option<Arc<auth::TokenStore>>,
    #[cfg(feature = "prometheus")]
    pub prometheus_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
}

/// Constant-time byte comparison to prevent timing attacks on auth credentials.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

async fn simple_basic_auth(
    expected_username: Option<&str>,
    expected_password: Option<&str>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Result<axum::response::Response> {
    let (expected_user, expected_pass) = match (expected_username, expected_password) {
        (Some(u), Some(p)) => (u, p),
        _ => return Ok(next.run(request).await),
    };
    let user_pass = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Basic "))
        .and_then(|v| base64::engine::general_purpose::STANDARD.decode(v).ok())
        .and_then(|v| String::from_utf8(v).ok());
    let user_pass = match user_pass {
        Some(user_pass) => user_pass,
        None => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [("WWW-Authenticate", "Basic realm=\"API\"")],
            )
                .into_response());
        }
    };
    // Use constant-time comparison to prevent timing attacks
    match user_pass.split_once(':') {
        Some((u, p))
            if constant_time_eq(u.as_bytes(), expected_user.as_bytes())
                && constant_time_eq(p.as_bytes(), expected_pass.as_bytes()) =>
        {
            Ok(next.run(request).await)
        }
        _ => Err(ApiError::unauthorized()),
    }
}

impl HttpApi {
    pub fn new(api: Api, opts: Option<HttpApiOptions>) -> Self {
        Self {
            api,
            opts: opts.unwrap_or_default(),
        }
    }

    /// Run the HTTP server forever on the given address.
    /// If read_only is passed, no state-modifying methods will be exposed.
    #[inline(never)]
    pub fn make_http_api_and_run(
        #[allow(unused_mut)] mut self,
        listener: TcpListener,
        upnp_router: Option<Router>,
    ) -> BoxFuture<'static, anyhow::Result<()>> {
        #[cfg(feature = "prometheus")]
        let mut prometheus_handle = self.opts.prometheus_handle.take();

        let state = Arc::new(self);

        let mut main_router = handlers::make_api_router(state.clone());

        #[cfg(feature = "webui")]
        {
            use axum::response::Redirect;

            let webui_router = webui::make_webui_router();
            main_router = main_router.nest("/web/", webui_router);
            main_router = main_router.route("/web", get(|| async { Redirect::permanent("./web/") }))
        }

        #[cfg(feature = "swagger")]
        {
            use utoipa::OpenApi;
            use utoipa_swagger_ui::SwaggerUi;
            main_router = main_router.merge(
                SwaggerUi::new("/swagger")
                    .url("/api-docs/openapi.json", ApiDoc::openapi()),
            );
        }

        #[cfg(feature = "prometheus")]
        if let Some(handle) = prometheus_handle.take() {
            let session = state.api.session().clone();
            main_router = main_router.route(
                "/metrics",
                get(move || async move {
                    let mut metrics = handle.render();
                    session.stats_snapshot().as_prometheus(&mut metrics);
                    metrics
                }),
            );
        }

        let cors_layer = {
            use tower_http::cors::{AllowHeaders, AllowOrigin};

            const ALLOWED_ORIGINS: [&[u8]; 4] = [
                // Webui-dev
                b"http://localhost:3031",
                b"http://127.0.0.1:3031",
                // Tauri dev
                b"http://localhost:1420",
                // Tauri prod
                b"tauri://localhost",
            ];

            let allow_regex = std::env::var("CORS_ALLOW_REGEXP")
                .ok()
                .and_then(|value| regex::bytes::Regex::new(&value).ok());

            tower_http::cors::CorsLayer::default()
                .allow_origin(AllowOrigin::predicate(move |v, _| {
                    ALLOWED_ORIGINS.contains(&v.as_bytes())
                        || allow_regex
                            .as_ref()
                            .map(move |r| r.is_match(v.as_bytes()))
                            .unwrap_or(false)
                }))
                .allow_headers(AllowHeaders::any())
        };

        // Unified auth: supports Bearer token and Basic auth
        if state.opts.basic_auth.is_some() {
            let token_store = state.opts.token_store.clone();
            let user = state.opts.basic_auth.as_ref().unwrap().0.clone();
            let pass = state.opts.basic_auth.as_ref().unwrap().1.clone();
            info!("Enabling authentication in HTTP API");
            main_router = main_router.route_layer(axum::middleware::from_fn(
                move |headers: HeaderMap, request: axum::extract::Request, next: Next| {
                    let token_store = token_store.clone();
                    let user = user.clone();
                    let pass = pass.clone();
                    async move {
                        // Skip auth for login/refresh endpoints
                        let path = request.uri().path();
                        if path == "/auth/login" || path == "/auth/refresh" {
                            return Ok(next.run(request).await);
                        }

                        // Try Bearer token first
                        if let Some(ts) = &token_store {
                            if let Some(token) = headers
                                .get("Authorization")
                                .and_then(|h| h.to_str().ok())
                                .and_then(|h| h.strip_prefix("Bearer "))
                            {
                                if ts.validate_access_token(token) {
                                    return Ok(next.run(request).await);
                                }
                                return Err(ApiError::unauthorized());
                            }
                        }

                        // Fall back to Basic auth
                        simple_basic_auth(Some(&user), Some(&pass), headers, request, next).await
                    }
                },
            ));
        }

        // qBittorrent WebUI API v2 compatibility layer.
        // Mounted after auth so it handles its own cookie-based auth.
        {
            let qbit_router = handlers::qbit_compat::make_qbit_router(state.clone());
            main_router = main_router.nest("/api/v2", qbit_router);
        }

        if let Some(upnp_router) = upnp_router {
            main_router = main_router.nest("/upnp", upnp_router);
        }

        let app = main_router
            .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024)) // 100 MB max request body
            .layer(cors_layer)
            .layer(
                tower_http::trace::TraceLayer::new_for_http()
                    .make_span_with(|req: &Request| {
                        let method = req.method();
                        let uri = req.uri();
                        if let Some(ConnectInfo(addr)) = req
                            .extensions()
                            .get::<ConnectInfo<librqbit_dualstack_sockets::WrappedSocketAddr>>()
                        {
                            debug_span!("request", %method, %uri, addr=%addr.0)
                        } else {
                            debug_span!("request", %method, %uri)
                        }
                    })
                    .on_request(|req: &Request, _: &Span| {
                        if req.uri().path().starts_with("/upnp") {
                            debug!(headers=?req.headers())
                        }
                    })
                    .on_response(DefaultOnResponse::new().include_headers(true))
                    .on_failure({
                        let mut default = DefaultOnFailure::new();
                        move |failure_class, latency, span: &Span| match failure_class {
                            tower_http::classify::ServerErrorsFailureClass::StatusCode(
                                StatusCode::NOT_IMPLEMENTED,
                            ) => {}
                            _ => default.on_failure(failure_class, latency, span),
                        }
                    }),
            )
            .into_make_service_with_connect_info::<librqbit_dualstack_sockets::WrappedSocketAddr>();

        async move {
            axum::serve(listener, app)
                .await
                .context("error running HTTP API")
        }
        .boxed()
    }
}
