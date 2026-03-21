//! Indexarr integration — proxy handlers that forward requests to an Indexarr instance.
//!
//! All endpoints return 404 when Indexarr integration is not enabled
//! (i.e. `state.opts.indexarr_url` is `None`).

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use http::StatusCode;
use serde_json::json;

use super::ApiState;

/// Shared helper: build a reqwest client and verify Indexarr is configured.
fn indexarr_client(
    state: &ApiState,
) -> Result<(reqwest::Client, String, Option<String>), impl IntoResponse> {
    match &state.opts.indexarr_url {
        Some(url) => Ok((
            reqwest::Client::new(),
            url.trim_end_matches('/').to_string(),
            state.opts.indexarr_api_key.clone(),
        )),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Indexarr integration is not enabled"})),
        )),
    }
}

/// Forward a GET request to Indexarr, injecting the API key.
async fn proxy_get(
    state: &ApiState,
    path: &str,
    params: &HashMap<String, String>,
) -> impl IntoResponse {
    let (client, base_url, api_key) = match indexarr_client(state) {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };

    let url = format!("{}{}", base_url, path);
    let mut req = client.get(&url).query(params);
    if let Some(key) = &api_key {
        req = req.header("X-Api-Key", key);
    }

    match req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let body = resp.text().await.unwrap_or_default();
            (status, [("Content-Type", "application/json")], body).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": format!("Failed to reach Indexarr: {e}")})),
        )
            .into_response(),
    }
}

/// Forward a POST request (JSON body) to Indexarr, injecting the API key.
async fn proxy_post_json(
    state: &ApiState,
    path: &str,
    body: serde_json::Value,
) -> impl IntoResponse {
    let (client, base_url, api_key) = match indexarr_client(state) {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };

    let url = format!("{}{}", base_url, path);
    let mut req = client.post(&url).json(&body);
    if let Some(key) = &api_key {
        req = req.header("X-Api-Key", key);
    }

    match req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let body = resp.text().await.unwrap_or_default();
            (status, [("Content-Type", "application/json")], body).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": format!("Failed to reach Indexarr: {e}")})),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Public handler functions
// ---------------------------------------------------------------------------

/// GET /indexarr/status — check if Indexarr is enabled and reachable.
pub async fn h_indexarr_status(State(state): State<ApiState>) -> impl IntoResponse {
    let Some(url) = &state.opts.indexarr_url else {
        return Json(json!({
            "enabled": false,
        }))
        .into_response();
    };

    let client = reqwest::Client::new();
    let mut req = client.get(format!("{}/health", url.trim_end_matches('/')));
    if let Some(key) = &state.opts.indexarr_api_key {
        req = req.header("X-Api-Key", key);
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or(json!({}));
            Json(json!({
                "enabled": true,
                "reachable": true,
                "indexarr": body,
            }))
            .into_response()
        }
        Ok(resp) => Json(json!({
            "enabled": true,
            "reachable": false,
            "error": format!("Indexarr returned status {}", resp.status()),
        }))
        .into_response(),
        Err(e) => Json(json!({
            "enabled": true,
            "reachable": false,
            "error": format!("{e}"),
        }))
        .into_response(),
    }
}

/// GET /indexarr/search — proxy to Indexarr search API.
pub async fn h_indexarr_search(
    State(state): State<ApiState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    proxy_get(&state, "/api/v1/search", &params).await
}

/// GET /indexarr/recent — proxy to Indexarr recent torrents.
pub async fn h_indexarr_recent(
    State(state): State<ApiState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    proxy_get(&state, "/api/v1/recent", &params).await
}

/// GET /indexarr/trending — proxy to Indexarr trending torrents.
pub async fn h_indexarr_trending(
    State(state): State<ApiState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    proxy_get(&state, "/api/v1/trending", &params).await
}

/// GET /indexarr/torrent/{info_hash} — proxy to Indexarr torrent detail.
pub async fn h_indexarr_torrent_detail(
    State(state): State<ApiState>,
    Path(info_hash): Path<String>,
) -> impl IntoResponse {
    proxy_get(
        &state,
        &format!("/api/v1/torrent/{info_hash}"),
        &HashMap::new(),
    )
    .await
}

/// GET /indexarr/identity/status — proxy to Indexarr identity status.
pub async fn h_indexarr_identity_status(
    State(state): State<ApiState>,
) -> impl IntoResponse {
    proxy_get(&state, "/api/v1/identity/status", &HashMap::new()).await
}

/// POST /indexarr/identity/acknowledge — proxy to Indexarr identity acknowledge.
pub async fn h_indexarr_identity_acknowledge(
    State(state): State<ApiState>,
) -> impl IntoResponse {
    proxy_post_json(&state, "/api/v1/identity/acknowledge", json!({})).await
}

/// GET /indexarr/sync/preferences — proxy to Indexarr sync preferences.
pub async fn h_indexarr_sync_preferences_get(
    State(state): State<ApiState>,
) -> impl IntoResponse {
    proxy_get(&state, "/api/v1/system/sync/preferences", &HashMap::new()).await
}

/// POST /indexarr/sync/preferences — proxy to Indexarr sync preferences.
pub async fn h_indexarr_sync_preferences_set(
    State(state): State<ApiState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    proxy_post_json(&state, "/api/v1/system/sync/preferences", body).await
}
