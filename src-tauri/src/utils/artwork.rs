use crate::config::constants::{APPLE_MUSIC_URL, ITUNES_SEARCH_API_URL};
use reqwest::blocking::Client;
use urlencoding::encode;

/// Search for the album artwork on iTunes
pub fn get_artwork_url(artist: &str, title: &str) -> Option<String> {
    let client = Client::new();

    // Build the query for iTunes API
    let query = format!("{} {}", artist, title);
    let encoded_query = encode(&query);
    let itunes_url = format!(
        "{}?term={}&media=music&limit=1",
        ITUNES_SEARCH_API_URL, encoded_query
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
                    return Some(larger_artwork);
                }
            }
        }
    }

    println!("No artwork found on iTunes");
    None
}

/// Generate search URL for Apple Music
pub fn get_apple_music_search_url(title: &str, artist: &str) -> String {
    let apple_music_query = format!("{} {}", title, artist);
    let encoded_query = encode(&apple_music_query);
    format!("{}/search?term={}", APPLE_MUSIC_URL, encoded_query)
}
