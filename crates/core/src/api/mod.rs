//! Slack Web API client.

mod auth;
pub mod chat;
mod client;
mod conversations;
mod emoji;
mod history;
mod internal;
mod pagination;
pub mod profile;
mod reactions;
mod rtm;
mod users;

pub use auth::AuthTest;
pub use chat::{PostMessage, Posted};
pub use client::{Client, DEFAULT_BASE_URL};
pub use conversations::{Conversation, ConversationKind, TextValue};
pub use history::{Edited, HistoryPage, HistoryQuery, Message, Reaction};
pub use internal::ReadMarker;
pub use profile::ClientProfile;
pub use rtm::RtmConnect;
pub use users::{Profile, User};

mod channel_details;
pub use channel_details::Pin;

mod search;
pub use search::{SearchMatch, SearchPage, SearchSort};

mod presence;
pub use presence::{CustomStatus, Dnd, SetPresence, UserPresence};
