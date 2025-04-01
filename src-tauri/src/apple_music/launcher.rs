use crate::apple_music::player;
use crate::config::constants::APPLE_MUSIC_URL;
use std::time::Duration;

/// Open Apple Music in app mode using a compatible browser
pub fn open_apple_music() {
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
            format!("--app={}", APPLE_MUSIC_URL),
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
            if let Err(e) = player::store_pid(pid) {
                eprintln!("Failed to store PID: {}", e);
                return;
            }

            // Wait a bit for the browser to fully initialize
            std::thread::sleep(Duration::from_secs(5));

            // Try to verify if MPRIS is working
            match player::find_apple_music_player() {
                Ok(player) => println!("Successfully verified MPRIS player: {}", player.identity()),
                Err(e) => println!(
                    "Note: Could not verify MPRIS player yet: {}. This is normal during startup.",
                    e
                ),
            }
        }
        Err(e) => {
            eprintln!("Failed to open Apple Music with {}: {}", browser_cmd, e);
        }
    }
}

/// Kill Apple Music process
pub fn kill_apple_music() {
    let player = player::find_apple_music_player();
    match player {
        Ok(_) => {
            // If we have the PID, use it to kill the process
            match player::get_pid() {
                Ok(pid) => {
                    println!("Killing Apple Music process with PID: {}", pid);
                    let _ = std::process::Command::new("kill")
                        .arg(pid.to_string())
                        .spawn();
                }
                Err(_) => {
                    // Fallback to pkill if we don't have the PID
                    let _ = std::process::Command::new("pkill")
                        .args(["-f", "chromium --app=https://music.apple.com"])
                        .spawn();
                }
            }
        }
        Err(_) => {
            // Fallback to pkill if we don't have a valid player
            let _ = std::process::Command::new("pkill")
                .args(["-f", "chromium --app=https://music.apple.com"])
                .spawn();
        }
    }
}
