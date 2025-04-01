pub mod apple_music;
pub mod commands;
pub mod config;
pub mod discord;
pub mod error;
pub mod ui;
pub mod utils;

/// Main entry point for the application
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize the Tauri application
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![commands::start_discord_presence])
        .setup(|app| {
            // Initialize Discord Rich Presence
            println!("Initializing Discord Rich Presence...");
            match commands::start_discord_presence() {
                Ok(msg) => println!("{}", msg),
                Err(e) => eprintln!("Failed to initialize Discord presence: {}", e),
            }

            // Setup the tray icon
            if let Err(e) = ui::setup_tray(app) {
                eprintln!("Failed to setup tray: {}", e);
            }

            // Open Apple Music on startup
            apple_music::open_apple_music();

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Error while running tauri application");
}
