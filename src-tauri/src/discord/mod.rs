pub mod client;

// Re-export commonly used functions
pub use client::{clear_presence, initialize, set_activity, start_periodic_updates};
