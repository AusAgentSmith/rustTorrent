use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

const ACCESS_TOKEN_TTL: Duration = Duration::from_secs(15 * 60); // 15 minutes
const REFRESH_TOKEN_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60); // 30 days

struct TokenEntry {
    expires_at: Instant,
}

pub struct TokenStore {
    access_tokens: RwLock<HashMap<String, TokenEntry>>,
    refresh_tokens: RwLock<HashMap<String, TokenEntry>>,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

fn generate_token() -> String {
    let bytes: [u8; 32] = rand::random();
    hex::encode(bytes)
}

impl TokenStore {
    pub fn new() -> Self {
        Self {
            access_tokens: RwLock::new(HashMap::new()),
            refresh_tokens: RwLock::new(HashMap::new()),
        }
    }

    pub fn create_tokens(&self) -> TokenResponse {
        let access_token = generate_token();
        let refresh_token = generate_token();
        let now = Instant::now();

        self.access_tokens.write().insert(
            access_token.clone(),
            TokenEntry {
                expires_at: now + ACCESS_TOKEN_TTL,
            },
        );
        self.refresh_tokens.write().insert(
            refresh_token.clone(),
            TokenEntry {
                expires_at: now + REFRESH_TOKEN_TTL,
            },
        );

        TokenResponse {
            access_token,
            refresh_token,
            token_type: "Bearer",
            expires_in: ACCESS_TOKEN_TTL.as_secs(),
        }
    }

    pub fn validate_access_token(&self, token: &str) -> bool {
        let tokens = self.access_tokens.read();
        tokens
            .get(token)
            .is_some_and(|entry| entry.expires_at > Instant::now())
    }

    pub fn refresh(&self, refresh_token: &str) -> Option<TokenResponse> {
        let valid = {
            let tokens = self.refresh_tokens.read();
            tokens
                .get(refresh_token)
                .is_some_and(|entry| entry.expires_at > Instant::now())
        };

        if !valid {
            return None;
        }

        // Revoke the old refresh token (rotation)
        self.refresh_tokens.write().remove(refresh_token);

        Some(self.create_tokens())
    }

    pub fn revoke_refresh_token(&self, refresh_token: &str) {
        self.refresh_tokens.write().remove(refresh_token);
    }

    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        self.access_tokens
            .write()
            .retain(|_, entry| entry.expires_at > now);
        self.refresh_tokens
            .write()
            .retain(|_, entry| entry.expires_at > now);
    }
}

use axum::{Json, extract::State, response::IntoResponse};
use http::StatusCode;

use super::HttpApi;

type ApiState = Arc<HttpApi>;

pub async fn h_auth_login(
    State(state): State<ApiState>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let (expected_user, expected_pass) = match &state.opts.basic_auth {
        Some((u, p)) => (u.as_str(), p.as_str()),
        None => {
            return (StatusCode::NOT_FOUND, "authentication not configured").into_response();
        }
    };

    if !super::constant_time_eq(req.username.as_bytes(), expected_user.as_bytes())
        || !super::constant_time_eq(req.password.as_bytes(), expected_pass.as_bytes())
    {
        return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
    }

    let token_store = match &state.opts.token_store {
        Some(ts) => ts,
        None => {
            return (StatusCode::NOT_FOUND, "token auth not configured").into_response();
        }
    };

    token_store.cleanup_expired();
    let tokens = token_store.create_tokens();
    (StatusCode::OK, Json(tokens)).into_response()
}

pub async fn h_auth_refresh(
    State(state): State<ApiState>,
    Json(req): Json<RefreshRequest>,
) -> impl IntoResponse {
    let token_store = match &state.opts.token_store {
        Some(ts) => ts,
        None => {
            return (StatusCode::NOT_FOUND, "token auth not configured").into_response();
        }
    };

    match token_store.refresh(&req.refresh_token) {
        Some(tokens) => (StatusCode::OK, Json(tokens)).into_response(),
        None => (StatusCode::UNAUTHORIZED, "invalid or expired refresh token").into_response(),
    }
}

pub async fn h_auth_logout(
    State(state): State<ApiState>,
    Json(req): Json<LogoutRequest>,
) -> impl IntoResponse {
    if let Some(ts) = &state.opts.token_store {
        ts.revoke_refresh_token(&req.refresh_token);
    }
    StatusCode::NO_CONTENT
}
