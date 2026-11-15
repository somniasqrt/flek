//! Flek core: auth, Slack API, storage and sync. No UI dependencies.

pub mod api;
pub mod auth;
pub mod error;
pub mod files;
pub mod rtm;
pub mod secret;
pub mod store;
pub mod sync;

pub use error::{Error, Result};
pub use secret::Secret;

pub mod references;
pub mod views;

pub mod reads;
pub mod time;

pub mod search;

pub mod rail;

pub mod settings;

mod net_policy;
mod private_fs;

/// Logging policy shared by executable entry points.
pub mod logging;

/// Agent write authorization, audit and rate limits.
pub mod agent;
pub mod notifications;

/// Prepared, guarded CLI write operations.
pub mod writes;
