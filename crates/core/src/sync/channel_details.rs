//! Persist a complete membership/pins snapshot before any view is produced.
use crate::{api::Client, store::Store, Result};
use rusqlite::params;

pub async fn fresh(client: &Client, store: &Store, id: &str) -> Result<()> {
    let members = client.conversations_members(id).await?;
    let pins = client.pins_list(id).await?;
    let id = id.to_owned();
    store
        .call(move |c| {
            // Pins carry partial messages: only fill gaps, as with archive import.
            let messages = pins
                .iter()
                .filter_map(|p| p.message.as_ref())
                .map(super::backfill::to_row)
                .collect::<Vec<_>>();

            let tx = c.transaction()?;
            crate::store::messages::import_messages_in_transaction(&tx, &id, &messages)?;
            tx.execute(
                "
                    DELETE
                    FROM conversation_members
                    WHERE conversation_id = ?1
                ",
                [&id],
            )?;
            tx.execute(
                "
                    DELETE
                    FROM pins
                    WHERE conversation_id = ?1
                ",
                [&id],
            )?;
            for user in members {
                tx.execute(
                    "
                        INSERT OR IGNORE INTO conversation_members
                        VALUES (?1, ?2)
                    ",
                    params![id, user],
                )?;
            }
            for pin in pins {
                if pin.kind == "message" {
                    if let Some(m) = pin.message {
                        tx.execute(
                            "
                                INSERT OR REPLACE INTO pins
                                VALUES (?1, ?2, ?3, ?4)
                            ",
                            params![id, m.ts, pin.created_by, pin.created],
                        )?;
                    }
                }
            }
            tx.execute(
                "
                    INSERT OR REPLACE INTO conversation_details_sync
                    VALUES (?1, unixepoch())
                ",
                [&id],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
}
