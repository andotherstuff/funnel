//! Bearer token authentication middleware.

use axum::{
    Json,
    body::Body,
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use subtle::ConstantTimeEq;

/// Configuration for bearer token authentication.
#[derive(Clone)]
pub struct AuthConfig {
    /// The expected bearer token value.
    token: String,
}

impl AuthConfig {
    /// Create a new auth config with the given token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    /// Create auth config from the API_TOKEN environment variable.
    ///
    /// Returns `None` if the environment variable is not set or is empty.
    pub fn from_env() -> Option<Self> {
        std::env::var("API_TOKEN")
            .ok()
            .filter(|s| !s.is_empty())
            .map(Self::new)
    }

    /// Validate a bearer token against the configured token.
    pub fn validate(&self, token: &str) -> bool {
        // Use constant-time comparison to prevent timing attacks
        let a = token.as_bytes();
        let b = self.token.as_bytes();

        // Length check is not constant-time, but that's acceptable for tokens
        // since the expected token length is not secret
        a.len() == b.len() && a.ct_eq(b).into()
    }
}

/// Extract bearer token from Authorization header.
fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

/// Authentication middleware that validates bearer tokens.
///
/// This middleware checks for a valid `Authorization: Bearer <token>` header
/// and rejects requests that don't have a valid token.
pub async fn require_auth(headers: HeaderMap, request: Request<Body>, next: Next) -> Response {
    // Get auth config from request extensions
    let auth_config = request
        .extensions()
        .get::<AuthConfig>()
        .expect("AuthConfig not found in request extensions");

    match extract_bearer_token(&headers) {
        Some(token) if auth_config.validate(token) => next.run(request).await,
        Some(_) => unauthorized_response("Invalid token"),
        None => unauthorized_response("Missing authorization header"),
    }
}

/// Generate a 401 Unauthorized response with a JSON error body.
fn unauthorized_response(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": message })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_config_validates_correct_token() {
        let config = AuthConfig::new("secret-token-123");
        assert!(config.validate("secret-token-123"));
    }

    #[test]
    fn auth_config_rejects_incorrect_token() {
        let config = AuthConfig::new("secret-token-123");
        assert!(!config.validate("wrong-token"));
    }

    #[test]
    fn auth_config_rejects_empty_token() {
        let config = AuthConfig::new("secret-token-123");
        assert!(!config.validate(""));
    }

    #[test]
    fn auth_config_rejects_different_length_token() {
        let config = AuthConfig::new("secret-token-123");
        assert!(!config.validate("short"));
        assert!(!config.validate("this-is-a-much-longer-token-than-expected"));
    }

    #[test]
    fn extract_bearer_token_parses_valid_header() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer my-token".parse().unwrap());
        assert_eq!(extract_bearer_token(&headers), Some("my-token"));
    }

    #[test]
    fn extract_bearer_token_returns_none_for_missing_header() {
        let headers = HeaderMap::new();
        assert_eq!(extract_bearer_token(&headers), None);
    }

    #[test]
    fn extract_bearer_token_returns_none_for_non_bearer_auth() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Basic dXNlcjpwYXNz".parse().unwrap());
        assert_eq!(extract_bearer_token(&headers), None);
    }

    #[test]
    fn from_env_returns_none_for_empty_string() {
        // Simulate empty API_TOKEN (as set by docker-compose with ${API_TOKEN:-})
        // SAFETY: This test runs single-threaded (--test-threads=1)
        unsafe {
            std::env::set_var("API_TOKEN", "");
        }
        assert!(AuthConfig::from_env().is_none());

        // Clean up
        unsafe {
            std::env::remove_var("API_TOKEN");
        }
    }

    #[test]
    fn from_env_returns_some_for_non_empty_token() {
        // SAFETY: This test runs single-threaded (--test-threads=1)
        unsafe {
            std::env::set_var("API_TOKEN", "test-token");
        }
        let config = AuthConfig::from_env();
        assert!(config.is_some());
        assert!(config.unwrap().validate("test-token"));

        // Clean up
        unsafe {
            std::env::remove_var("API_TOKEN");
        }
    }
}
