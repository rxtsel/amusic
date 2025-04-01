use crate::config::constants::DISCORD_CLIENT_ID;
use crate::error::{AppError, Result};
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

lazy_static::lazy_static! {
    static ref DISCORD_CLIENT: Mutex<Option<DiscordIpcClient>> = Mutex::new(None);
}

/// Initialize Discord client
pub fn initialize() -> Result<String> {
    let mut client_guard = lock_client()?;

    if client_guard.is_none() {
        let mut client = DiscordIpcClient::new(DISCORD_CLIENT_ID)
            .map_err(|e| AppError::Discord(format!("Error creating Discord client: {}", e)))?;

        // Connect to Discord
        client
            .connect()
            .map_err(|e| AppError::Discord(format!("Error connecting to Discord: {}", e)))?;

        *client_guard = Some(client);
        println!("Discord client initialized and connected");
    }

    Ok("Discord presence initialized".to_string())
}

/// Lock the Discord client mutex
pub fn lock_client() -> Result<MutexGuard<'static, Option<DiscordIpcClient>>> {
    DISCORD_CLIENT
        .lock()
        .map_err(|e| AppError::Discord(format!("Failed to lock Discord client mutex: {}", e)))
}

/// Clear Discord rich presence
pub fn clear_presence() -> Result<()> {
    let mut client_guard = lock_client()?;

    if let Some(ref mut client) = *client_guard {
        client
            .clear_activity()
            .map_err(|e| AppError::Discord(format!("Error clearing activity: {}", e)))?;
        println!("Discord presence cleared");
    }

    Ok(())
}

/// Update Discord rich presence with current track information
pub fn set_activity(
    title: &str,
    artist: &str,
    artwork_url: Option<&str>,
    start_time: i64,
    end_time: Option<i64>,
    apple_music_url: &str,
) -> Result<()> {
    let mut client_guard = lock_client()?;

    if let Some(ref mut client) = *client_guard {
        // Assets for Discord activity
        let mut assets = activity::Assets::new()
            .small_image("amusic_lg")
            .small_text("Apple Music");

        // Add artwork if available
        if let Some(url) = artwork_url {
            assets = assets.large_image(url);
        } else {
            assets = assets.large_image("amusic_lg");
        }

        // Create button for Apple Music
        let button = activity::Button::new("Play in Apple Music", apple_music_url);

        // Create timestamps with start time and initial end time of 0
        let mut timestamps = activity::Timestamps::new()
            .start(start_time)
            .end(start_time);

        // Update with real data when available
        if let Some(end) = end_time {
            if end > start_time && end - start_time <= 86400 {
                // Ensure end time is reasonable (less than 24 hours)
                timestamps = timestamps.end(end);
                println!(
                    "Using actual song duration for Discord presence: {} seconds",
                    end - start_time
                );
            } else {
                println!("Received invalid end time, keeping initial end time");
            }
        } else {
            println!("No end time available yet, using initial value");
        }

        // Update Discord activity
        client
            .set_activity(
                activity::Activity::new()
                    .details(title)
                    .state(artist)
                    .assets(assets)
                    .activity_type(activity::ActivityType::Listening)
                    .buttons(vec![button])
                    .timestamps(timestamps),
            )
            .map_err(|e| AppError::Discord(format!("Error setting presence: {}", e)))?;

        println!("Discord presence updated: {} - {}", artist, title);
    } else {
        return Err(AppError::Discord("Discord client not initialized".into()));
    }

    Ok(())
}

/// Schedule periodic updates for Discord presence
pub fn start_periodic_updates() {
    std::thread::spawn(|| {
        // Wait a bit before starting updates
        std::thread::sleep(Duration::from_secs(5));
        println!("Starting Discord presence polling thread");

        loop {
            match crate::apple_music::player::update_discord_presence() {
                Ok(msg) => println!("Polling update: {}", msg),
                Err(e) => {
                    // Only show errors that aren't expected during initialization
                    if !e.to_string().contains("PID not stored")
                        && !e.to_string().contains("not found")
                    {
                        println!("Polling update error: {}", e);
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(10)); // Adjustable interval
        }
    });
}
