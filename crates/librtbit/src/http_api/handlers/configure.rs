use axum::{Json, extract::State, response::IntoResponse};

use super::ApiState;
use crate::{
    api::{EmptyJsonResponse, Result},
    limits::LimitsConfig,
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
