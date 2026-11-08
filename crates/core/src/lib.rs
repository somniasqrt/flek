//! Flek core: auth, Slack API, storage and sync. No UI dependencies.

pub mod api;
pub mod auth;
pub mod error;
pub mod secret;

pub use error::{Error, Result};
pub use secret::Secret;
