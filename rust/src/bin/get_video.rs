use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use scraper::{Html, Selector};

use dbz::{save_episodes, Episode, EpisodeMap};

const BASE_URL: &str = "https://anizone.to/anime/99nxtlfu/{}";
const START_EPISODE: u32 = 1;
const END_EPISODE: u32 = 291;

fn get_video_data(page_url: &str) -> Result<serde_json::Value> {
    let client = reqwest::blocking::Client::new();

    let response = client
        .get(page_url)
        .header("User-Agent", "Mozilla/5.0")
        .timeout(Duration::from_secs(15))
        .send()
        .context("request failed")?;

    if !response.status().is_success() {
        bail!("non-success status: {}", response.status());
    }

    let html = response.text().context("failed to read body")?;

    let document = Html::parse_document(&html);

    let x_data_selector = Selector::parse("[x-data]").unwrap();

    let mut player = None;

    for element in document.select(&x_data_selector) {
        if let Some(value) = element.value().attr("x-data") {
            if value.contains("vidstackPlayer") {
                player = Some(value.to_string());
                break;
            }
        }
    }

    let x_data = player.context("Video configuration not found")?;

    let start = x_data
        .find("JSON.parse('")
        .map(|i| i + "JSON.parse('".len())
        .context("JSON.parse data not found")?;

    let end = x_data[start..]
        .find("')")
        .map(|i| start + i)
        .context("End of JSON data not found")?;

    let encoded_json = &x_data[start..end];

    let decoded_json = encoded_json.replace("\\/", "/");
    let decoded_json = decoded_json.replace("\\u0022", "\"");

    let value = serde_json::from_str(&decoded_json)
        .context("failed to parse embedded JSON")?;

    Ok(value)
}

fn main() -> Result<()> {
    let mut results: EpisodeMap = EpisodeMap::new();

    for episode in START_EPISODE..=END_EPISODE {
        let url = BASE_URL.replace("{}", &episode.to_string());

        println!("[{episode}/{END_EPISODE}] Fetching episode {episode}...");

        match get_video_data(&url) {
            Ok(data) => {
                let video = data
                    .get("src")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let subtitles: Vec<dbz::Subtitle> = data
                    .get("subtitles")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                serde_json::from_value(item.clone()).ok()
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                results.insert(
                    episode.to_string(),
                    Episode {
                        page: Some(url.clone()),
                        video,
                        subtitles,
                        ..Default::default()
                    },
                );

                println!("  ✓ Found video");
                println!("  ✓ Subtitles: {}", results[&episode.to_string()].subtitles.len());
            }
            Err(e) => {
                println!("  ✗ Error: {e}");

                results.insert(
                    episode.to_string(),
                    Episode {
                        page: Some(url),
                        error: Some(e.to_string()),
                        ..Default::default()
                    },
                );
            }
        }

        thread::sleep(Duration::from_millis(500));
    }

    save_episodes(&results)?;

    println!("\nDone!");
    println!("Saved to episodes.json");

    Ok(())
}