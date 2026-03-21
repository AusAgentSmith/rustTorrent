use axum::{Json, extract::State, response::IntoResponse};
use axum::extract::Path;

use super::ApiState;
use crate::{
    api::{EmptyJsonResponse, Result, TorrentIdOrHash},
    limits::{
        AltSpeedConfig, AltSpeedSchedule, AltSpeedStatus, AltSpeedToggle, LimitsConfig,
        TorrentLimitsConfig,
    },
};

#[cfg_attr(feature = "swagger", utoipa::path(
    post,
    path = "/torrents/limits",
    request_body(content = LimitsConfig, description = "Rate limits configuration"),
    responses(
        (status = 200, description = "Rate limits updated", body = EmptyJsonResponse)
    )
))]
pub async fn h_update_session_ratelimits(
    State(state): State<ApiState>,
    Json(limits): Json<LimitsConfig>,
) -> Result<impl IntoResponse> {
    state
        .api
        .session()
        .ratelimits
        .set_upload_bps(limits.upload_bps);
    state
        .api
        .session()
        .ratelimits
        .set_download_bps(limits.download_bps);
    if let Some(peer_limit) = limits.peer_limit {
        state.api.session().set_peer_limit(peer_limit);
    }
    if let Some(init_limit) = limits.concurrent_init_limit {
        state.api.session().set_concurrent_init_limit(init_limit);
    }
    Ok(Json(EmptyJsonResponse {}))
}

#[cfg_attr(feature = "swagger", utoipa::path(
    get,
    path = "/torrents/limits",
    responses(
        (status = 200, description = "Current rate limits", body = LimitsConfig)
    )
))]
pub async fn h_get_session_ratelimits(State(state): State<ApiState>) -> Result<impl IntoResponse> {
    let mut config = state.api.session().ratelimits.get_config();
    config.peer_limit = Some(state.api.session().get_peer_limit());
    config.concurrent_init_limit = Some(state.api.session().get_concurrent_init_limit());
    Ok(Json(config))
}

// --- Per-torrent rate limits ---

pub async fn h_get_torrent_limits(
    State(state): State<ApiState>,
    Path(id): Path<TorrentIdOrHash>,
) -> Result<impl IntoResponse> {
    let handle = state.api.mgr_handle(id)?;
    let live = handle.live();
    let config = if let Some(live) = live {
        TorrentLimitsConfig {
            download_rate: live.ratelimits.get_download_bps().map(|v| v.get()),
            upload_rate: live.ratelimits.get_upload_bps().map(|v| v.get()),
        }
    } else {
        // If not live, return the configured (static) limits
        let opts = &handle.shared().options;
        TorrentLimitsConfig {
            download_rate: opts.ratelimits.download_bps.map(|v| v.get()),
            upload_rate: opts.ratelimits.upload_bps.map(|v| v.get()),
        }
    };
    Ok(Json(config))
}

pub async fn h_set_torrent_limits(
    State(state): State<ApiState>,
    Path(id): Path<TorrentIdOrHash>,
    Json(limits): Json<TorrentLimitsConfig>,
) -> Result<impl IntoResponse> {
    let handle = state.api.mgr_handle(id)?;
    if let Some(live) = handle.live() {
        live.ratelimits.set_download_bps(limits.download_bps());
        live.ratelimits.set_upload_bps(limits.upload_bps());
    }
    Ok(Json(EmptyJsonResponse {}))
}

// --- Alternative speed mode ---

pub async fn h_get_alt_speed(State(state): State<ApiState>) -> Result<impl IntoResponse> {
    let session = state.api.session();
    let status = AltSpeedStatus {
        enabled: session.alt_speed.is_enabled(),
        config: session.alt_speed.config(),
    };
    Ok(Json(status))
}

pub async fn h_set_alt_speed(
    State(state): State<ApiState>,
    Json(toggle): Json<AltSpeedToggle>,
) -> Result<impl IntoResponse> {
    let session = state.api.session();
    if toggle.enabled {
        session.alt_speed.enable(&session.ratelimits);
    } else {
        session.alt_speed.disable(&session.ratelimits);
    }
    Ok(Json(EmptyJsonResponse {}))
}

// --- Alternative speed schedule ---

pub async fn h_get_alt_speed_schedule(
    State(state): State<ApiState>,
) -> Result<impl IntoResponse> {
    let schedule = state.api.session().alt_speed.schedule();
    Ok(Json(schedule))
}

pub async fn h_set_alt_speed_schedule(
    State(state): State<ApiState>,
    Json(schedule): Json<AltSpeedSchedule>,
) -> Result<impl IntoResponse> {
    state.api.session().alt_speed.set_schedule(schedule);
    Ok(Json(EmptyJsonResponse {}))
}

// --- Alternative speed config ---

pub async fn h_set_alt_speed_config(
    State(state): State<ApiState>,
    Json(config): Json<AltSpeedConfig>,
) -> Result<impl IntoResponse> {
    let session = state.api.session();
    session.alt_speed.set_config(config);
    // If alt speed is currently enabled, re-apply the new limits
    if session.alt_speed.is_enabled() {
        let cfg = session.alt_speed.config();
        session.ratelimits.set_upload_bps(cfg.alt_speed_up);
        session.ratelimits.set_download_bps(cfg.alt_speed_down);
    }
    Ok(Json(EmptyJsonResponse {}))
}
