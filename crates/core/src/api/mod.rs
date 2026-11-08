//! Slack Web API client.

mod auth;
mod client;

pub use auth::AuthTest;
pub use client::{Client, DEFAULT_BASE_URL};
