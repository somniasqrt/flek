use flek_core::store::{messages, Store};

#[test]
fn local_date_lookup_respects_day_bounds_archive_edge_and_timeline_visibility() {
    let store = fixture();
    store.with_read(|c| {
        assert!(matches!(messages::date_target(c, "C1", "1699990000.000000", "1699999999.000000")?, messages::DateTarget::BeforeArchive));
        assert!(matches!(messages::date_target(c, "C1", "1700000000.000001", "1700000000.000004")?, messages::DateTarget::Found { ts } if ts == "1700000000.000003"));
        assert!(matches!(messages::date_target(c, "C1", "1700000000.000001", "1700000000.000003")?, messages::DateTarget::Empty));
        assert!(matches!(messages::date_target(c, "missing", "1700000000.000000", "1800000000.000000")?, messages::DateTarget::Empty));
        assert!(matches!(messages::date_target(c, "C1", "1800000000.000000", "1900000000.000000")?, messages::DateTarget::Empty));
        Ok(())
    }).unwrap();
}

#[test]
fn target_window_is_bounded_sorted_and_forward_pages_do_not_duplicate() {
    let store = fixture();
    store
        .with_read(|c| {
            let window = messages::timeline_window(c, "C1", "1700000000.000004", 4)?;
            assert_eq!(
                window
                    .rows
                    .iter()
                    .map(|m| m.ts.as_str())
                    .collect::<Vec<_>>(),
                [
                    "1700000000.000000",
                    "1700000000.000003",
                    "1700000000.000004",
                    "1700000000.000005"
                ]
            );
            assert!(!window.has_older);
            assert!(window.has_newer);
            let next = messages::timeline_after(c, "C1", &window.rows.last().unwrap().ts, 4)?;
            assert_eq!(next.len(), 2);
            assert_eq!(next[0].ts, "1700000000.000006");
            let later = messages::timeline_window(c, "C1", "1700000000.000006", 4)?;
            assert!(later.has_older);
            assert!(!later.has_newer);
            assert!(
                messages::timeline_window(c, "missing", "1700000000.000004", 4)?
                    .rows
                    .is_empty()
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn loaded_interval_re_read_includes_bounds_and_excludes_deleted_and_reply_only_rows() {
    let store = fixture();
    store
        .with_read(|c| {
            let rows = messages::timeline_range(c, "C1", "1700000000.000001", "1700000000.000005")?;
            assert_eq!(
                rows.iter().map(|m| m.ts.as_str()).collect::<Vec<_>>(),
                [
                    "1700000000.000003",
                    "1700000000.000004",
                    "1700000000.000005"
                ]
            );
            assert!(messages::timeline_range(c, "missing", "0", "9")?.is_empty());
            Ok(())
        })
        .unwrap();
}

#[test]
fn block_images_share_the_existing_cache_and_unsupported_attachments_are_flagged() {
    use flek_core::store::unfurls;
    let store = fixture();
    store.with_conn(|c| {
        let blocks = serde_json::json!([
            {"type":"section", "text":{"type":"plain_text", "text":"fixture"}, "accessory":{"type":"image", "image_url":"https://files.slack-edge.com/fixture.png", "alt_text":"image"}},
            {"type":"context", "elements":[{"type":"image", "image_url":"https://files.slack-edge.com/icon.png", "alt_text":"icon"}]},
            {"type":"image", "image_url":"file:///outside.png", "alt_text":"not fetched"}
        ]);
        let raw = serde_json::json!({"blocks": blocks, "attachments":[{"actions":[{"type":"button"}]}]}).to_string();
        c.execute("UPDATE messages SET raw_json = ?1, blocks_json = ?2 WHERE conversation_id = 'C1' AND ts = '1700000000.000000'", rusqlite::params![raw, blocks.to_string()])?;
        assert_eq!(unfurls::images_to_fetch(c, 10)?.len(), 2);
        assert_eq!(unfurls::images_to_fetch(c, 1)?.len(), 1);
        unfurls::set_image(c, "https://files.slack-edge.com/fixture.png", Some("/synthetic/cache/fixture.png"))?;
        let row = messages::message(c, "C1", "1700000000.000000")?.unwrap();
        assert_eq!(row.block_images.len(), 1);
        assert!(row.unsupported);
        assert_eq!(row.block_images["https://files.slack-edge.com/fixture.png"], unfurls::url_hash("https://files.slack-edge.com/fixture.png"));
        assert_eq!(unfurls::images_to_fetch(c, 10)?, ["https://files.slack-edge.com/icon.png"]);
        assert!(!unfurls::unsupported_attachments(r#"{"attachments":[{"text":"supported"}]}"#));
        assert!(unfurls::unsupported_attachments(r#"{"attachments":[{"future":"unknown"}]}"#));
        Ok(())
    }).unwrap();
}

fn fixture() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Demo');
            INSERT INTO conversations (id, team_id, kind) VALUES ('C1', 'T1', 'public_channel');",
            )?;
            for i in 0..8 {
                let ts = format!("1700000000.{i:06}");
                c.execute(
                    "INSERT INTO messages (conversation_id, ts, text, deleted, thread_ts, subtype)
                VALUES ('C1', ?1, 'fixture', ?2, ?3, ?4)",
                    rusqlite::params![
                        ts,
                        i == 2,
                        (i == 1 || i == 3).then_some("1700000000.000000"),
                        (i == 3).then_some("thread_broadcast")
                    ],
                )?;
            }
            Ok(())
        })
        .unwrap();
    store
}
