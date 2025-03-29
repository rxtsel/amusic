// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use mpris::{Event, PlayerFinder};
use reqwest::blocking::Client;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use urlencoding::encode;

use lazy_static::lazy_static;

// Discord application client ID - change this if needed
const DISCORD_CLIENT_ID: &str = "XXXXXXXXXXXXXXXXXXX";

// URL for Apple Music
const APPLE_MUSIC_URL: &str = "https://music.apple.com";

lazy_static! {
    static ref DISCORD_CLIENT: Mutex<Option<DiscordIpcClient>> = Mutex::new(None);
}

/// Search for the album artwork on iTunes
fn get_artwork_url(artist: &str, title: &str) -> Option<String> {
    let client = Client::new();

    // Build the query for iTunes API
    let query = format!("{} {}", artist, title);
    let encoded_query = encode(&query);
    let itunes_url = format!(
        "https://itunes.apple.com/search?term={}&media=music&limit=1",
        encoded_query
    );

    // Make the request
    let response = match client.get(&itunes_url).send() {
        Ok(resp) => resp,
        Err(e) => {
            println!("Error making request to iTunes: {}", e);
            return None;
        }
    };

    // Analyze the response
    if let Ok(json) = response.json::<serde_json::Value>() {
        if let Some(results) = json["results"].as_array() {
            if !results.is_empty() {
                if let Some(artwork_url) = results[0]["artworkUrl100"].as_str() {
                    // Get a larger version by replacing 100x100 with 600x600
                    let larger_artwork = artwork_url.replace("100x100", "600x600");
                    println!("Artwork found: {}", larger_artwork);
                    return Some(larger_artwork);
                }
            }
        }
    }

    println!("No artwork found on iTunes");
    None
}

#[tauri::command]
fn start_discord_presence() -> Result<String, String> {
    // Initialize the Discord client only once at startup
    let mut discord_initialized = false;

    {
        let mut client_guard = DISCORD_CLIENT
            .lock()
            .map_err(|e| format!("Failed to lock Discord client mutex: {}", e))?;

        if client_guard.is_none() {
            let mut client = DiscordIpcClient::new(DISCORD_CLIENT_ID)
                .map_err(|e| format!("Error creating Discord client: {e}"))?;

            // Connect to Discord
            if let Err(e) = client.connect() {
                return Err(format!("Error connecting to Discord: {e}"));
            }

            *client_guard = Some(client);
            discord_initialized = true;
            println!("Discord client initialized and connected");
        }
    }

    // Only spawn the threads if we're initializing for the first time
    if discord_initialized {
        // Thread para escuchar eventos MPRIS
        thread::spawn(|| {
            loop {
                if let Err(e) = listen_for_player_events() {
                    eprintln!("Error in player events listener: {}", e);
                    // Wait a bit before trying again
                    thread::sleep(Duration::from_secs(3));
                }
            }
        });

        // Thread adicional de polling para actualizar la presencia periódicamente
        thread::spawn(|| {
            loop {
                match update_discord_presence() {
                    Ok(msg) => println!("Polling update: {}", msg),
                    Err(e) => println!("Polling update error: {}", e),
                }
                thread::sleep(Duration::from_secs(10)); // Intervalo ajustable
            }
        });
    }

    // Intentar actualizar la presencia con el estado actual del reproductor, si hay alguno
    match update_discord_presence() {
        Ok(msg) => Ok(msg),
        Err(e) => {
            println!("No active player or media found on startup: {}", e);
            Ok("Discord presence initialized. Waiting for media playback...".to_string())
        }
    }
}

// Function to update Discord presence based on current player state
fn update_discord_presence() -> Result<String, String> {
    let finder = PlayerFinder::new().map_err(|e| format!("Error creating PlayerFinder: {e}"))?;
    let player = finder
        .find_active()
        .map_err(|e| format!("No active player found: {e}"))?;

    // Check if player is actually playing something
    let playback_status = player
        .get_playback_status()
        .map_err(|e| format!("Error getting playback status: {e}"))?;

    // Only update presence if player is playing
    if playback_status != mpris::PlaybackStatus::Playing {
        // Clear presence if not playing
        let mut client_guard = DISCORD_CLIENT
            .lock()
            .map_err(|e| format!("Failed to lock Discord client mutex: {}", e))?;

        if let Some(ref mut client) = *client_guard {
            let _ = client.clear_activity();
        }

        return Err("Player is not currently playing".into());
    }

    let metadata = player
        .get_metadata()
        .map_err(|e| format!("Error getting metadata: {e}"))?;

    let title = metadata.title().unwrap_or("No title").to_string();
    let artist = metadata.artists().unwrap_or(vec!["Unknown"])[0].to_string();
    let album = metadata.album_name().unwrap_or("Without album").to_string();

    // Get the song duration and current position
    let length_micros = metadata.length().unwrap_or_default().as_micros();
    let position = player.get_position().unwrap_or_default().as_micros();

    // Calculate the start and end times
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // Convert from microseconds to seconds
    let position_seconds = position / 1_000_000;
    let remaining_seconds = (length_micros.saturating_sub(position)) / 1_000_000;

    // Calculate the start time (when the song started playing)
    let start_time = current_time - position_seconds as i64;

    // Calculate the end time
    let end_time = current_time + remaining_seconds as i64;

    // Assets para la actividad en Discord
    let mut assets = activity::Assets::new()
        .small_image("amusic_lg")
        .small_text("AMusic");

    // Intentamos encontrar la portada del álbum en línea mediante la API de iTunes
    let artwork_url = get_artwork_url(&artist, &title);
    if let Some(ref online_url) = artwork_url {
        assets = assets.large_image(online_url);
    } else {
        // Si no se encuentra la imagen en línea, se usa la imagen por defecto
        assets = assets.large_image("amusic_lg");
    }

    // Crear URL de búsqueda para Apple Music
    let apple_music_query = format!("{} {}", title, artist);
    let encoded_query = encode(&apple_music_query);
    let apple_music_url = format!("https://music.apple.com/search?term={}", encoded_query);

    // Crear el botón para Apple Music
    let button = activity::Button::new("Play in Apple Music", &apple_music_url);

    // Crear timestamps para mostrar tiempo transcurrido y restante
    let timestamps = activity::Timestamps::new().start(start_time).end(end_time);

    // Actualizar la actividad en Discord
    let mut client_guard = DISCORD_CLIENT
        .lock()
        .map_err(|e| format!("Failed to lock Discord client mutex: {}", e))?;

    if let Some(ref mut client) = *client_guard {
        client
            .set_activity(
                activity::Activity::new()
                    .state(&format!("Album: {album}"))
                    .details(&format!("{artist} - {title}"))
                    .assets(assets)
                    .activity_type(activity::ActivityType::Listening)
                    .buttons(vec![button])
                    .timestamps(timestamps),
            )
            .map_err(|e| format!("Error setting presence: {e}"))?;
    } else {
        return Err("Discord client not initialized".into());
    }

    println!("Discord presence updated: {artist} - {title}");
    Ok(format!("Discord presence active: {artist} - {title}"))
}

// Function to listen for MPRIS events and update Discord presence accordingly
fn listen_for_player_events() -> Result<(), String> {
    let finder = PlayerFinder::new().map_err(|e| format!("Error creating PlayerFinder: {e}"))?;

    // Try to find an active player
    let player = finder
        .find_active()
        .map_err(|e| format!("Error finding active player: {e}"))?;

    // Watch for events from this player
    let player_name = player.identity().to_string();
    println!("Monitoring player: {}", player_name);

    // Track if we're currently playing
    let mut is_playing = player
        .get_playback_status()
        .map(|status| status == mpris::PlaybackStatus::Playing)
        .unwrap_or(false);

    // Actualizamos inmediatamente la presencia si ya está reproduciendo
    if is_playing {
        println!("Player is already playing, updating presence...");
        let _ = update_discord_presence();
    }

    // Obtener el stream de eventos del reproductor
    let events = player
        .events()
        .map_err(|e| format!("Error getting player events: {e}"))?;

    println!("Successfully connected to player events stream");

    for event_result in events {
        println!("Received event from player: {:?}", event_result);

        if let Ok(event) = event_result {
            match event {
                Event::Playing => {
                    println!("Event: Player started playing");
                    is_playing = true;
                    let _ = update_discord_presence();
                }
                Event::Paused | Event::Stopped => {
                    println!("Event: Player paused or stopped");
                    is_playing = false;
                    let mut client_guard = DISCORD_CLIENT
                        .lock()
                        .map_err(|e| format!("Failed to lock Discord client mutex: {}", e))?;

                    if let Some(ref mut client) = *client_guard {
                        if let Err(e) = client.clear_activity() {
                            println!("Error clearing activity: {:?}", e);
                        } else {
                            println!("Discord presence cleared");
                        }
                    }
                }
                Event::TrackChanged(_) => {
                    println!("Event: Track changed");
                    if is_playing {
                        let _ = update_discord_presence();
                    }
                }
                Event::PlayerShutDown => {
                    println!("Event: Player shut down");
                    let mut client_guard = DISCORD_CLIENT
                        .lock()
                        .map_err(|e| format!("Failed to lock Discord client mutex: {}", e))?;

                    if let Some(ref mut client) = *client_guard {
                        if let Err(e) = client.clear_activity() {
                            println!("Error clearing activity: {:?}", e);
                        } else {
                            println!("Discord presence cleared due to player shutdown");
                        }
                    }
                    return Ok(());
                }
                _ => {
                    println!("Unhandled event: {:?}", event);
                }
            }
        } else if let Err(e) = event_result {
            println!("Error handling player event: {:?}", e);
        }
    }

    Err("Player events stream ended unexpectedly".into())
}

// Function to open Apple Music in chromium app mode
fn open_apple_music() {
    println!("Opening Apple Music in app mode...");

    // Simply launch a new instance
    println!("Opening new Apple Music instance");
    if let Err(e) = std::process::Command::new("chromium")
        .args([
            "--app=".to_string() + APPLE_MUSIC_URL,
            "--class=AppleMusic".to_string(),
        ])
        .spawn()
    {
        eprintln!("Failed to open Apple Music: {}", e);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![start_discord_presence])
        .setup(|app| {
            println!("Initializing Discord Rich Presence...");
            match start_discord_presence() {
                Ok(msg) => println!("{}", msg),
                Err(e) => eprintln!("Failed to initialize Discord presence: {}", e),
            }

            // Create tray menu items - only quit option
            let quit_i = MenuItem::with_id(app, "quit", "Quit Apple Music", true, None::<&str>)
                .expect("Failed to create 'Quit' menu item");

            // Create tray menu with just the quit item
            let menu = Menu::with_items(app, &[&quit_i]).expect("Failed to create tray menu");

            // Create the tray icon with menu
            let _tray = TrayIconBuilder::new()
                .icon(
                    app.default_window_icon()
                        .expect("Failed to get default window icon")
                        .clone(),
                )
                .tooltip("Apple Music")
                .menu(&menu)
                // Always show the menu on right click
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        println!("Quit menu item clicked");
                        // Find and kill any chromium --app instances first
                        let _ = std::process::Command::new("pkill")
                            .args(["-f", "chromium --app=https://music.apple.com"])
                            .spawn();

                        // Then exit the app
                        app.exit(0);
                    }
                    _ => {
                        println!("Unhandled menu item: {:?}", event.id);
                    }
                })
                .build(app)
                .expect("Failed to create tray icon");

            // Open Apple Music on startup
            open_apple_music();

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
