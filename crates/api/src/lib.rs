//! Funnel REST API Library
//!
//! Provides handlers and router configuration for the video analytics API.

pub mod auth;
pub mod handlers;
pub mod router;

#[cfg(test)]
mod tests;

pub use self::auth::AuthConfig;
pub use self::handlers::*;
pub use self::router::create_router;
