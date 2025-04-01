use crate::discord;
use crate::error::{AppError, Result};
use crate::utils::artwork;
use mpris::{Event, PlaybackStatus, Player, PlayerFinder};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

// Store our Apple Music process PID
lazy_static::lazy_static! {
    static ref APPLE_MUSIC_PID: Mutex<Option<u32>> = Mutex::new(None);
}

/// Store the PID of the Apple Music process
pub fn store_pid(pid: u32) -> Result<()> {
    match APPLE_MUSIC_PID.lock() {
        Ok(mut pid_guard) => {
            println!("Storing Apple Music PID {} in global variable", pid);
            *pid_guard = Some(pid);
            Ok(())
        }
        Err(e) => Err(AppError::Application(format!(
            "Failed to lock PID mutex: {}",
            e
        ))),
    }
}

/// Get the stored Apple Music PID
pub(crate) fn get_pid() -> Result<u32> {
    match APPLE_MUSIC_PID.lock() {
        Ok(guard) => match *guard {
            Some(pid) => Ok(pid),
            None => Err(AppError::Player("Apple Music PID not stored".into())),
        },
        Err(e) => Err(AppError::Application(format!(
            "Failed to lock PID mutex: {}",
            e
        ))),
    }
}

/// Find the Apple Music player instance using MPRIS
pub fn find_apple_music_player() -> Result<Player> {
    // Get our stored PID
    let apple_music_pid = get_pid()?;
    println!(
        "Looking for Apple Music player with PID: {}",
        apple_music_pid
    );

    // Find all players and match by PID
    let finder = PlayerFinder::new()
        .map_err(|e| AppError::Mpris(format!("Error creating PlayerFinder: {}", e)))?;

    let players = finder
        .find_all()
        .map_err(|e| AppError::Mpris(format!("Error finding players: {}", e)))?;

    println!("Found {} players via MPRIS", players.len());

    // If no players, return error
    if players.is_empty() {
        return Err(AppError::Player(format!(
            "Apple Music player with PID {} not found. No players are active yet.",
            apple_music_pid
        )));
    }

    // Search exclusively by PID
    let pid_str = apple_music_pid.to_string();

    // Search directly by D-Bus name containing our PID
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

            // Return the player directly
            return Ok(player);
        }
    }

    // If we don't find our player with specific PID, return an error
    Err(AppError::Player(format!(
        "Apple Music player with PID {} not found. No player for this specific instance is active yet.",
        apple_music_pid
    )))
}

/// Function to update Discord presence based on current player state
pub fn update_discord_presence() -> Result<String> {
    // Find our specific Apple Music player
    println!("Updating Discord presence - looking for our Apple Music player...");
    let player = match find_apple_music_player() {
        Ok(p) => p,
        Err(e) => {
            // If we don't find our specific player, return the error
            println!("Could not find our specific Apple Music player: {}", e);
            return Err(e);
        }
    };

    println!("Found player: {}", player.identity());

    // Check if player is actually playing something
    let playback_status = player
        .get_playback_status()
        .map_err(|e| AppError::Mpris(format!("Error getting playback status: {}", e)))?;

    // Only update presence if player is playing
    if playback_status != PlaybackStatus::Playing {
        // Clear presence if not playing
        discord::clear_presence()?;
        return Err(AppError::Player("Player is not currently playing".into()));
    }

    let metadata = player
        .get_metadata()
        .map_err(|e| AppError::Mpris(format!("Error getting metadata: {}", e)))?;

    let title = metadata.title().unwrap_or("No title").to_string();
    let artist = metadata.artists().unwrap_or(vec!["Unknown"])[0].to_string();

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

    // Validate that the position is reasonable
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

    // Try to find album cover online using iTunes API
    let artwork_url = artwork::get_artwork_url(&artist, &title);

    // Create search URL for Apple Music
    let apple_music_url = artwork::get_apple_music_search_url(&title, &artist);

    // Update Discord activity
    discord::set_activity(
        &title,
        &artist,
        artwork_url.as_deref(),
        start_time,
        end_time,
        &apple_music_url,
    )?;

    Ok(format!("Discord presence active: {} - {}", artist, title))
}

/// Function to listen for MPRIS events and update Discord presence accordingly
pub fn listen_for_player_events() -> Result<()> {
    // Check first if we have a stored PID
    let apple_music_pid = get_pid()?;
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
            let playing = status == PlaybackStatus::Playing;
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
    let events = player
        .events()
        .map_err(|e| AppError::Mpris(format!("Error getting player events: {}", e)))?;

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
                    let _ = discord::clear_presence();
                }
                Event::TrackChanged(_) => {
                    println!("Event: Track changed");
                    if is_playing {
                        let _ = update_discord_presence();
                    }
                }
                Event::PlayerShutDown => {
                    println!("Event: Player shut down");
                    let _ = discord::clear_presence();
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

    Err(AppError::Player(
        "Player events stream ended unexpectedly".into(),
    ))
}

/// Start the event listener thread for MPRIS events
pub fn start_event_listener() {
    thread::spawn(|| {
        // Wait a bit before starting to listen for events
        thread::sleep(Duration::from_secs(3));
        println!("Starting MPRIS event listener thread");

        loop {
            if let Err(e) = listen_for_player_events() {
                // Only print errors that aren't due to unstored PID
                if !e.to_string().contains("PID not stored") {
                    eprintln!("Error in player events listener: {}", e);
                }
                // Wait a bit before trying again
                thread::sleep(Duration::from_secs(3));
            }
        }
    });
}
