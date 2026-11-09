//! App commands go through Tauri's permission system, so only windows whose
//! capability lists them (the main window) can call them; the sign-in window
//! showing Slack's page gets none.

const COMMANDS: &[&str] = &[
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
    "open_file",
    "pdf_data",
    "open_url",
    "search",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
