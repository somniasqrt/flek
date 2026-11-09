//! Confirm with Slack, then apply the same idempotent change as the RTM echo.
use crate::{
    api::Client,
    rtm::Event,
    store::Store,
    sync::live::{self, Applied},
    Result,
};

pub async fn set(
    client: &Client,
    store: &Store,
    user: &str,
    channel: &str,
    ts: &str,
    name: &str,
    on: bool,
) -> Result<Applied> {
    client.set_message_reaction(channel, ts, name, on).await?;
    let event = if on {
        Event::ReactionAdded {
            channel: channel.into(),
            ts: ts.into(),
            name: name.into(),
            user: user.into(),
        }
    } else {
        Event::ReactionRemoved {
            channel: channel.into(),
            ts: ts.into(),
            name: name.into(),
            user: user.into(),
        }
    };
    live::apply(store, event).await
}
