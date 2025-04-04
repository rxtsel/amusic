pub mod launcher;
pub mod player;

// Re-export commonly used functions
pub use launcher::{kill_apple_music, open_apple_music};
pub use player::{start_event_listener, update_discord_presence};
