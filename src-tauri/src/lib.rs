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
const DISCORD_CLIENT_ID: &str = "1354665491792138350";

// URL for Apple Music
const APPLE_MUSIC_URL: &str = "https://music.apple.com";

lazy_static! {
    static ref DISCORD_CLIENT: Mutex<Option<DiscordIpcClient>> = Mutex::new(None);
    // Store our Apple Music process PID
    static ref APPLE_MUSIC_PID: Mutex<Option<u32>> = Mutex::new(None);
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

    // Verify we have a valid Apple Music PID before starting threads
    let apple_music_pid_valid = match APPLE_MUSIC_PID.lock() {
        Ok(guard) => guard.is_some(),
        Err(_) => false,
    };

    if !apple_music_pid_valid {
        println!("WARNING: No Apple Music PID stored yet. Starting threads anyway...");
    } else {
        println!("Apple Music PID is valid, proceeding with thread initialization");
    }

    // Only spawn the threads if we're initializing for the first time
    if discord_initialized {
        // Thread for listening to MPRIS events
        thread::spawn(|| {
            // Wait a bit before starting to listen for events
            thread::sleep(Duration::from_secs(3));
            println!("Starting MPRIS event listener thread");

            loop {
                if let Err(e) = listen_for_player_events() {
                    // Only print errors that aren't due to unstored PID
                    if !e.contains("PID not stored") {
                        eprintln!("Error in player events listener: {}", e);
                    }
                    // Wait a bit before trying again
                    thread::sleep(Duration::from_secs(3));
                }
            }
        });

        // Additional polling thread to update presence periodically
        thread::spawn(|| {
            // Wait a bit before starting updates
            thread::sleep(Duration::from_secs(5));
            println!("Starting Discord presence polling thread");

            loop {
                match update_discord_presence() {
                    Ok(msg) => println!("Polling update: {}", msg),
                    Err(e) => {
                        // Only show errors that aren't expected during initialization
                        if !e.contains("PID not stored") && !e.contains("not found") {
                            println!("Polling update error: {}", e);
                        }
                    }
                }
                thread::sleep(Duration::from_secs(10)); // Adjustable interval
            }
        });
    }

    // Try to update presence with current player state, if any
    match update_discord_presence() {
        Ok(msg) => Ok(msg),
        Err(e) => {
            println!("No active player or media found on startup: {}", e);
            Ok("Discord presence initialized. Waiting for media playback...".to_string())
        }
    }
}

// Function to find our Apple Music player based on known PID
fn find_apple_music_player() -> Result<mpris::Player, String> {
    // Get our stored PID
    let apple_music_pid = match APPLE_MUSIC_PID.lock() {
        Ok(guard) => match *guard {
            Some(pid) => pid,
            None => return Err("Apple Music PID not stored".into()),
        },
        Err(_) => return Err("Failed to lock APPLE_MUSIC_PID mutex".into()),
    };

    println!(
        "Looking for Apple Music player with PID: {}",
        apple_music_pid
    );

    // Find all players and match by PID
    let finder = PlayerFinder::new().map_err(|e| format!("Error creating PlayerFinder: {e}"))?;
    let players = finder
        .find_all()
        .map_err(|e| format!("Error finding players: {e}"))?;

    println!("Found {} players via MPRIS", players.len());

    // If no players, return error
    if players.is_empty() {
        return Err(format!(
            "Apple Music player with PID {} not found. No players are active yet.",
            apple_music_pid
        ));
    }

    // Search exclusively by PID
    let pid_str = apple_music_pid.to_string();

    // Method 1: Search directly by D-Bus name containing our PID
    for player in players {
        // Get the D-Bus name
        let bus_name = player.bus_name();
        println!(
            "Examining player: {} (bus name: {})",
            player.identity(),
            bus_name
        );

        // Check if bus_name contains our PID
        if bus_name.contains(&pid_str) {
            println!(
                "Found AppleMusic instance with PID {}: {}",
                apple_music_pid, bus_name
            );
            println!("Identity: {}", player.identity());

            // Additional debugging info
            if let Ok(metadata) = player.get_metadata() {
                println!("  Metadata available:");
                if let Some(url) = metadata.url() {
                    println!("  URL: {}", url);
                }

                if let Some(title) = metadata.title() {
                    println!("  Title: {}", title);
                }

                if let Some(artists) = metadata.artists() {
                    println!("  Artists: {}", artists.join(", "));
                }
            }

            // Return the player directly
            return Ok(player);
        }
    }

    // If we don't find our player with specific PID, return an error
    Err(format!("Apple Music player with PID {} not found. No player for this specific instance is active yet.", apple_music_pid))
}

// Function to update Discord presence based on current player state
fn update_discord_presence() -> Result<String, String> {
    // Find our specific Apple Music player
    println!("Updating Discord presence - looking for our Apple Music player...");
    let player = match find_apple_music_player() {
        Ok(p) => p,
        Err(e) => {
            // If we don't find our specific player, return the error
            println!("Could not find our specific Apple Music player: {}", e);
            return Err(format!("Our Apple Music player not found: {}", e));
        }
    };

    println!("Found player: {}", player.identity());

    // Check if player is actually playing something
    let playback_status = match player.get_playback_status() {
        Ok(status) => status,
        Err(e) => {
            println!("Error getting playback status: {}. Clearing presence.", e);
            // Clear presence in case of error
            if let Ok(mut client_guard) = DISCORD_CLIENT.lock() {
                if let Some(ref mut client) = *client_guard {
                    let _ = client.clear_activity();
                }
            }
            return Err(format!("Error getting playback status: {e}"));
        }
    };

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
    let album_text = &format!("Album: {album}");

    let length = metadata.length().unwrap_or_default();

    // Validate that the duration is reasonable (less than 24 hours in seconds)
    // If it's not a reasonable value, use a default value of 0 minutes
    let length_seconds = if length.as_secs() > 86400 {
        println!(
            "Invalid duration detected: {} seconds. Using default value.",
            length.as_secs()
        );
        0 // 0 minutes as default value
    } else {
        length.as_secs() as i64
    };

    // Get the current position using the MPRIS method
    let position = player.get_position().unwrap_or_default();

    // Validate that the position is reasonable (less than the total duration)
    let position_seconds = if position.as_secs() > length.as_secs() || position.as_secs() > 86400 {
        println!(
            "Invalid position detected: {} seconds. Using default value.",
            position.as_secs()
        );
        0 // Starting from the beginning as default value
    } else {
        position.as_secs() as i64
    };

    // Calculate timestamps for Discord
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // Calculate when the song started playing
    let start_time = current_time - position_seconds;

    // Calculate when the song will end
    let end_time = start_time + length_seconds;

    // Assets for Discord activity
    let mut assets = activity::Assets::new()
        .small_image("amusic_lg")
        .small_text("Apple Music")
        .large_text(album_text);

    // Try to find album cover online using iTunes API
    let artwork_url = get_artwork_url(&artist, &title);
    if let Some(ref online_url) = artwork_url {
        assets = assets.large_image(online_url);
    } else {
        // If no image found online, use default image
        assets = assets.large_image("amusic_lg");
    }

    // Create search URL for Apple Music
    let apple_music_query = format!("{} {}", title, artist);
    let encoded_query = encode(&apple_music_query);
    let apple_music_url = format!("https://music.apple.com/search?term={}", encoded_query);

    // Create button for Apple Music
    let button = activity::Button::new("Play in Apple Music", &apple_music_url);

    // Create timestamps to show elapsed and remaining time
    let timestamps = activity::Timestamps::new().start(start_time).end(end_time);

    // Update Discord activity
    let mut client_guard = DISCORD_CLIENT
        .lock()
        .map_err(|e| format!("Failed to lock Discord client mutex: {}", e))?;

    if let Some(ref mut client) = *client_guard {
        client
            .set_activity(
                activity::Activity::new()
                    .details(&title)
                    .state(&artist)
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
    // Check first if we have a stored PID
    let apple_music_pid = match APPLE_MUSIC_PID.lock() {
        Ok(guard) => match *guard {
            Some(pid) => pid,
            None => {
                println!("No Apple Music PID stored yet. Waiting for browser launch...");
                std::thread::sleep(Duration::from_secs(5));
                return Err("Apple Music PID not stored yet".into());
            }
        },
        Err(_) => return Err("Failed to lock APPLE_MUSIC_PID mutex".into()),
    };

    println!("Using stored Apple Music PID: {}", apple_music_pid);

    // Try to find our specific player
    println!("Attempting to find Apple Music player for event listening...");
    let player = match find_apple_music_player() {
        Ok(p) => p,
        Err(e) => {
            // If we can't find the player, wait a bit and return the error
            // so that the main loop tries again
            println!(
                "Could not find Apple Music player: {}. Waiting before retry...",
                e
            );
            std::thread::sleep(Duration::from_secs(5));
            return Err(e);
        }
    };

    // Watch for events from this player
    let player_name = player.identity().to_string();
    println!("Monitoring player: {}", player_name);

    // Check if player is playing
    let mut is_playing = match player.get_playback_status() {
        Ok(status) => {
            let playing = status == mpris::PlaybackStatus::Playing;
            println!(
                "Initial playback status: {}",
                if playing { "Playing" } else { "Not playing" }
            );
            playing
        }
        Err(e) => {
            println!(
                "Error getting initial playback status: {}. Assuming not playing.",
                e
            );
            false
        }
    };

    // Update presence immediately if already playing
    if is_playing {
        println!("Player is already playing, updating presence...");
        match update_discord_presence() {
            Ok(msg) => println!("{}", msg),
            Err(e) => println!("Could not update Discord presence: {}", e),
        }
    }

    // Get player events stream
    println!("Setting up event listener for player: {}", player_name);
    let events = match player.events() {
        Ok(e) => e,
        Err(e) => {
            println!("Error getting player events: {}. Will retry...", e);
            return Err(format!("Error getting player events: {e}"));
        }
    };

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

fn open_apple_music() {
    println!("Opening Apple Music in app mode...");

    // Determine which browser to use (chromium or brave)
    let browsers = ["chromium", "brave", "brave-browser"];
    let mut browser_cmd = String::new();

    for browser in browsers {
        // Check if browser is installed
        if let Ok(output) = std::process::Command::new("which").arg(browser).output() {
            if !output.stdout.is_empty() {
                browser_cmd = browser.to_string();
                println!("Found browser: {}", browser_cmd);
                break;
            }
        }
    }

    if browser_cmd.is_empty() {
        eprintln!("No compatible browser found. Please install Chromium or Brave.");
        return;
    }

    // Launch a new instance and store the child process
    println!("Opening new Apple Music instance with {}", browser_cmd);
    match std::process::Command::new(&browser_cmd)
        .args([
            "--app=".to_string() + APPLE_MUSIC_URL,
            "--no-first-run".to_string(),
            "--class=AppleMusic".to_string(),
            // Add additional arguments to improve MPRIS compatibility
            "--enable-features=MediaSessionService".to_string(),
        ])
        .spawn()
    {
        Ok(child) => {
            // Store the PID of our Apple Music instance
            let pid = child.id();
            println!("Apple Music launched with PID: {}", pid);

            // Store the PID in our global variable thread-safely
            {
                if let Ok(mut pid_guard) = APPLE_MUSIC_PID.lock() {
                    println!("Storing Apple Music PID {} in global variable", pid);
                    *pid_guard = Some(pid);
                } else {
                    eprintln!("Failed to lock APPLE_MUSIC_PID mutex");
                }
            }

            // Verify it was stored correctly
            {
                if let Ok(pid_guard) = APPLE_MUSIC_PID.lock() {
                    if let Some(stored_pid) = *pid_guard {
                        println!("Verified PID storage: {} is stored correctly", stored_pid);
                    } else {
                        eprintln!("ERROR: PID storage verification failed!");
                    }
                }
            }

            // Wait a bit for the browser to fully initialize
            println!("Waiting for browser to initialize...");
            std::thread::sleep(Duration::from_secs(5));

            // Try to verify if MPRIS is working
            match find_apple_music_player() {
                Ok(player) => println!("Successfully verified MPRIS player: {}", player.identity()),
                Err(e) => println!(
                    "Note: Could not verify MPRIS player yet: {}. This is normal during startup.",
                    e
                ),
            }

            // Let the child process continue running independently
            // No need to call `child.wait()` as we want it to run in background
        }
        Err(e) => {
            eprintln!("Failed to open Apple Music with {}: {}", browser_cmd, e);
        }
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

                        // Clear Discord presence before exiting
                        if let Ok(mut client_guard) = DISCORD_CLIENT.lock() {
                            if let Some(ref mut client) = *client_guard {
                                let _ = client.clear_activity();
                                println!("Discord presence cleared");
                            }
                        }

                        // Find and kill our specific Apple Music instance if PID is known
                        if let Ok(pid_guard) = APPLE_MUSIC_PID.lock() {
                            if let Some(pid) = *pid_guard {
                                println!("Killing Apple Music process with PID: {}", pid);
                                let _ = std::process::Command::new("kill")
                                    .arg(pid.to_string())
                                    .spawn();
                            } else {
                                // Fallback to pkill if we don't have the PID
                                let _ = std::process::Command::new("pkill")
                                    .args(["-f", "chromium --app=https://music.apple.com"])
                                    .spawn();
                            }
                        }

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
