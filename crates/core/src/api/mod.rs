//! Slack Web API client.

mod auth;
mod client;
mod conversations;
mod pagination;
mod users;

pub use auth::AuthTest;
pub use client::{Client, DEFAULT_BASE_URL};
pub use conversations::{Conversation, ConversationKind, TextValue};
pub use users::{Profile, User};
