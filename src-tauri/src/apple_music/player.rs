use crate::discord;
use crate::error::{AppError, Result};
use crate::utils::artwork;
use mpris::{Event, PlaybackStatus, Player, PlayerFinder, ProgressTick};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

    // Find all players and match by PID
    let finder = PlayerFinder::new()
        .map_err(|e| AppError::Mpris(format!("Error creating PlayerFinder: {}", e)))?;

    let players = finder
        .find_all()
        .map_err(|e| AppError::Mpris(format!("Error finding players: {}", e)))?;

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

        // Check if bus_name contains our PID
        if bus_name.contains(&pid_str) {
            println!(
                "Found AppleMusic instance with PID {}: {}",
                apple_music_pid, bus_name
            );

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

// Structure to cache song information
#[derive(Clone, Debug)]
struct SongInfo {
    title: String,
    artist: String,
    start_time: i64,
    end_time: Option<i64>,
    artwork_url: Option<String>,
    apple_music_url: String,
    last_updated: Instant,
}

// Global cache for song information
lazy_static::lazy_static! {
    static ref CURRENT_SONG: Mutex<Option<SongInfo>> = Mutex::new(None);
}

/// Get cached song info if available and still current
fn get_cached_song_info(title: &str, artist: &str) -> Option<SongInfo> {
    if let Ok(guard) = CURRENT_SONG.lock() {
        if let Some(song) = guard.as_ref() {
            // Check if it's the same song and cache is still fresh (less than 30 seconds old)
            if song.title == title
                && song.artist == artist
                && song.last_updated.elapsed() < Duration::from_secs(30)
            {
                return Some(song.clone());
            }
        }
    }
    None
}

/// Cache song information for later use
fn cache_song_info(song_info: SongInfo) -> Result<()> {
    match CURRENT_SONG.lock() {
        Ok(mut guard) => {
            *guard = Some(song_info);
            Ok(())
        }
        Err(e) => Err(AppError::Application(format!(
            "Failed to lock song cache mutex: {}",
            e
        ))),
    }
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

    // Create a progress tracker to get accurate position information
    let mut progress_tracker = player
        .track_progress(100) // Update every 100ms
        .map_err(|e| AppError::Mpris(format!("Error creating progress tracker: {}", e)))?;

    // Get current progress with accurate timing information
    let ProgressTick { progress, .. } = progress_tracker.tick();

    // Check if player is actually playing something
    if progress.playback_status() != PlaybackStatus::Playing {
        // Clear presence if not playing
        discord::clear_presence()?;
        return Err(AppError::Player("Player is not currently playing".into()));
    }

    let metadata = progress.metadata();
    let title = metadata.title().unwrap_or("No title").to_string();
    let artist = metadata.artists().unwrap_or(vec!["Unknown"])[0].to_string();

    // Get song duration and position from progress
    let position = progress.position().as_secs() as i64;
    let length = progress
        .length()
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);

    // Calculate when the song started playing
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let start_time = current_time - position;

    // Determine end_time based on song length
    let end_time = if length > 0 && length < 86400 {
        // Less than 24 hours
        // Only set end_time if we have a reasonable length
        Some(start_time + length)
    } else {
        None
    };

    // Check if we have cached info for this song
    if let Some(mut cached_song) = get_cached_song_info(&title, &artist) {
        println!("Using cached song information for {} - {}", artist, title);

        // Always update end_time if we have a valid one now
        if cached_song.end_time.is_none() && end_time.is_some() {
            println!(
                "Updating end time with newly available information: {:?}",
                end_time
            );
            cached_song.end_time = end_time;

            // Update the cache with the new end_time
            let updated_song = SongInfo {
                title: cached_song.title.clone(),
                artist: cached_song.artist.clone(),
                start_time: cached_song.start_time,
                end_time,
                artwork_url: cached_song.artwork_url.clone(),
                apple_music_url: cached_song.apple_music_url.clone(),
                last_updated: Instant::now(),
            };
            let _ = cache_song_info(updated_song);
        }

        // Update Discord with cached information
        discord::set_activity(
            &cached_song.title,
            &cached_song.artist,
            cached_song.artwork_url.as_deref(),
            cached_song.start_time,
            cached_song.end_time,
            &cached_song.apple_music_url,
        )?;

        return Ok(format!(
            "Discord presence active (cached): {} - {}",
            artist, title
        ));
    }

    // We already calculated these values above, so we can reuse them:
    // - start_time
    // - end_time

    if end_time.is_some() {
        println!("Valid song length detected: {} seconds", length);
    } else {
        println!(
            "Invalid song length: {} seconds. Using None for end_time initially.",
            length
        );
    }

    // Try to find album cover online using iTunes API
    let artwork_url = artwork::get_artwork_url(&artist, &title);

    // Create search URL for Apple Music
    let apple_music_url = artwork::get_apple_music_search_url(&title, &artist);

    // Cache this song information
    let song_info = SongInfo {
        title: title.clone(),
        artist: artist.clone(),
        start_time,
        end_time,
        artwork_url: artwork_url.clone(),
        apple_music_url: apple_music_url.clone(),
        last_updated: Instant::now(),
    };
    cache_song_info(song_info)?;

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
