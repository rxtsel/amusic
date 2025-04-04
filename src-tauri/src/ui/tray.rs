use crate::apple_music;
use crate::discord;
use crate::error::Result;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    App,
};

/// Setup tray icon and menu
pub fn setup(app: &App) -> Result<()> {
    // Create tray menu items - only quit option
    let quit_item = MenuItem::with_id(app, "quit", "Quit Apple Music", true, None::<&str>)
        .expect("Failed to create 'Quit' menu item");

    // Create tray menu with just the quit item
    let menu = Menu::with_items(app, &[&quit_item]).expect("Failed to create tray menu");

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
                let _ = discord::clear_presence();

                // Kill Apple Music process
                apple_music::kill_apple_music();

                // Then exit the app
                app.exit(0);
            }
            _ => {
                println!("Unhandled menu item: {:?}", event.id);
            }
        })
        .build(app)
        .expect("Failed to create tray icon");

    Ok(())
}
