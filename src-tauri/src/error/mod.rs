use std::fmt;

/// Custom error type for the application
#[derive(Debug)]
pub enum AppError {
    /// Discord-related errors
    Discord(String),
    /// MPRIS-related errors
    Mpris(String),
    /// Player-related errors
    Player(String),
    /// Network-related errors
    Network(String),
    /// General application errors
    Application(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Discord(msg) => write!(f, "Discord error: {}", msg),
            AppError::Mpris(msg) => write!(f, "MPRIS error: {}", msg),
            AppError::Player(msg) => write!(f, "Player error: {}", msg),
            AppError::Network(msg) => write!(f, "Network error: {}", msg),
            AppError::Application(msg) => write!(f, "Application error: {}", msg),
        }
    }
}

impl std::error::Error for AppError {}

/// Standard Result type for the application
pub type Result<T> = std::result::Result<T, AppError>;
