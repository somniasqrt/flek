//! Thin CLI adapters for the core's database read operations.
use crate::{default_team, format_ts, open_db, output::emit, Global};
use anyhow::Result;
use flek_core::{
    auth::{self, CredentialStore},
    views,
};
use flek_core::{reads, time};
use flek_core::{
    references::{self, Match},
    store::Store,
};
use serde_json::json;

mod args;
mod dispatch;
mod history;
pub use args::*;
pub use dispatch::run;
use history::{conv, message_read, print_user, times};
pub use history::{history, print_messages};
