//! App commands go through Tauri's permission system, so only windows whose
//! capability lists them (the main window) can call them; the sign-in window
//! showing Slack's page gets none.

const COMMANDS: &[&str] = &[
    "channel_directory",
    "channel_preview",
    "join_channel",
    "create_channel",
    "validate_channel_name",
    "get_settings",
    "set_settings",
    "storage_info",
    "clear_file_cache",
    "full_resync",
    "sign_out",
    "account_origin",
    "show_storage",
    "team_icon",
    "mark_read",
    "conversation_preference",
    "dismiss_conversation",
    "channel_info",
    "conversation_link",
    "direct_messages",
    "activity",
    "browse_files",
    "fetch_file",
    "status",
    "progress",
    "conversations",
    "messages",
    "message",
    "thread",
    "import_desktop",
    "sign_in",
    "people",
    "avatars",
    "custom_emoji",
    "profile",
    "profile_photo",
    "open_file",
    "pdf_data",
    "open_url",
    "search",
    "send_message",
    "set_reaction",
    "presence_for",
    "statuses_for",
    "subscribe_presence",
    "set_status",
    "clear_status",
    "set_presence",
    "snooze",
    "end_snooze",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
