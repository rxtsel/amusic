use crate::discord;

/// Tauri command to start Discord presence
#[tauri::command]
pub fn start_discord_presence() -> std::result::Result<String, String> {
    // Initialize the Discord client
    match discord::initialize() {
        Ok(_) => {
            // Start periodic updates thread
            discord::start_periodic_updates();

            // Start MPRIS event listener thread
            crate::apple_music::player::start_event_listener();

            // Try to update presence with current player state, if any
            match crate::apple_music::player::update_discord_presence() {
                Ok(msg) => Ok(msg),
                Err(e) => {
                    println!("No active player or media found on startup: {}", e);
                    Ok("Discord presence initialized. Waiting for media playback...".to_string())
                }
            }
        }
        Err(e) => Err(format!("Failed to initialize Discord: {}", e)),
    }
}
