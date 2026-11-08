//! Slack Web API client.

mod auth;
mod client;
mod conversations;
mod history;
mod pagination;
mod users;

pub use auth::AuthTest;
pub use client::{Client, DEFAULT_BASE_URL};
pub use conversations::{Conversation, ConversationKind, TextValue};
pub use history::{Edited, HistoryPage, HistoryQuery, Message};
pub use users::{Profile, User};
