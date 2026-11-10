//! Stable public facade; implementation grouped by responsibility.

mod conversations;
pub use conversations::*;
mod users;
pub use users::*;
mod messages;
pub use messages::*;
mod text;
pub use text::*;
mod version;
pub(crate) use users::user_row;
pub use version::*;
