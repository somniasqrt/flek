//! Thin IPC adapters. Command names and registration paths stay stable here.

mod conversations;
pub use conversations::*;
mod messages;
pub use messages::*;
mod presence;
pub use presence::*;
mod files;
pub use files::*;
mod settings;
pub use settings::*;
mod account;
pub use account::*;
mod channels;
pub use channels::*;

/// Errors cross IPC as text; core errors never carry secrets.
type CmdResult<T> = Result<T, String>;

fn text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Largest page the UI may ask for.
const MAX_PAGE: usize = 500;
