//! Slack Web API client.

mod auth;
mod client;
mod conversations;
mod history;
mod internal;
mod pagination;
pub mod profile;
mod rtm;
mod users;

pub use auth::AuthTest;
pub use client::{Client, DEFAULT_BASE_URL};
pub use conversations::{Conversation, ConversationKind, TextValue};
pub use history::{Edited, HistoryPage, HistoryQuery, Message, Reaction};
pub use internal::ReadMarker;
pub use profile::ClientProfile;
pub use rtm::RtmConnect;
pub use users::{Profile, User};
