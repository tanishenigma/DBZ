use std::collections::BTreeMap;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use regex::Regex;
use scraper::{Html, Selector};

use dbz::{load_episodes, save_episodes};

fn extract_metadata(page_url: &str) -> Result<BTreeMap<String, String>> {
    let client = reqwest::blocking::Client::new();

    let response = client
        .get(page_url)
        .header("User-Agent", "Mozilla/5.0")
        .timeout(Duration::from_secs(15))
        .send()
        .context("request failed")?;

    if !response.status().is_success() {
        anyhow::bail!("non-success status: {}", response.status());
    }

    let html = response.text().context("failed to read body")?;

    let document = Html::parse_document(&html);

    let mut data: BTreeMap<String, String> = BTreeMap::new();

    // -----------------------------------------
    // EPISODE HEADING
    // -----------------------------------------

    let h1_selector = Selector::parse("h1").unwrap();

    if let Some(heading) = document.select(&h1_selector).next() {
        let title = heading.text().collect::<Vec<_>>().join(" ").trim().to_string();
        // AniZone currently gives: "- Episode 1"
        data.insert("title".to_string(), title.trim_start_matches("- ").trim().to_string());

        // -----------------------------------------
        // FIND MAIN EPISODE INFORMATION
        // -----------------------------------------

        if let Some(parent) = heading.parent() {
            let parent = scraper::ElementRef::wrap(parent);
            let text = parent
                .map(|p| p.text().collect::<Vec<_>>().join("\n"))
                .unwrap_or_default();
            let lines: Vec<String> = text
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();

            // TYPE
            for value in ["Regular", "Special"] {
                if lines.iter().any(|l| l == value) {
                    data.insert("type".to_string(), value.to_string());
                    break;
                }
            }

            // DATE
            let date_re = Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap();
            for line in &lines {
                if let Some(m) = date_re.find(line) {
                    data.insert("air_date".to_string(), m.as_str().to_string());
                    break;
                }
            }
        }
    }

    // -----------------------------------------
    // SUMMARY
    // -----------------------------------------

    let p_selector = Selector::parse("p").unwrap();

    for paragraph in document.select(&p_selector) {
        let summary = paragraph.text().collect::<Vec<_>>().join(" ").trim().to_string();

        if summary.is_empty() {
            continue;
        }

        let lower = summary.to_lowercase();
        if lower.contains("intrusive ads") || lower.contains("disable your adblock") {
            continue;
        }

        if summary.chars().count() >= 40 {
            data.insert("summary".to_string(), summary);
            break;
        }
    }

    // -----------------------------------------
    // PAGE TEXT (for duration/source/audio/softsub)
    // -----------------------------------------

    let lines: Vec<String> = document
        .root_element()
        .text()
        .collect::<Vec<_>>()
        .join("\n")
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let duration_re = Regex::new(r"^\d+:\d+$").unwrap();

    for (i, line) in lines.iter().enumerate() {
        if line == "Duration:" {
            if let Some(value) = lines.get(i + 1) {
                if duration_re.is_match(value) {
                    data.insert("duration".to_string(), value.clone());
                }
            }
        }
    }

    for (i, line) in lines.iter().enumerate() {
        if line == "Source:" {
            if let Some(value) = lines.get(i + 1) {
                data.insert("source".to_string(), value.clone());
            }
        }
    }

    for (i, line) in lines.iter().enumerate() {
        if line == "Audio:" {
            if let Some(value) = lines.get(i + 1) {
                data.insert("audio".to_string(), value.clone());
            }
        }
    }

    for (i, line) in lines.iter().enumerate() {
        if line == "Softsub:" {
            if let Some(value) = lines.get(i + 1) {
                data.insert("softsub".to_string(), value.clone());
            }
        }
    }

    Ok(data)
}

fn fetch_episode(
    episode: String,
    episode_data: &dbz::Episode,
) -> (String, BTreeMap<String, String>, Option<String>) {
    let page = match &episode_data.page {
        Some(p) => p.clone(),
        None => return (episode, BTreeMap::new(), Some("no page URL".to_string())),
    };

    match extract_metadata(&page) {
        Ok(metadata) => (episode, metadata, None),
        Err(e) => (episode, BTreeMap::new(), Some(e.to_string())),
    }
}

fn main() -> Result<()> {
    let mut episodes = load_episodes()?;

    let total = episodes.len();

    println!("Fetching {total} episodes...");

    // 20 simultaneous requests.
    let (tx, rx) = mpsc::channel();

    let mut handles = Vec::new();

    for (number, data) in episodes.iter() {
        let tx = tx.clone();
        let number = number.clone();
        let data = data.clone();
        handles.push(thread::spawn(move || {
            let result = fetch_episode(number.clone(), &data);
            let _ = tx.send((number, result));
        }));
    }

    drop(tx);

    let mut completed = 0;

    for received in rx {
        let (number, (_, metadata, error)) = received;

        completed += 1;

        if let Some(err) = error {
            println!("[{completed}/{total}] {number}: ERROR {err}");
            continue;
        }

        if let Some(episode) = episodes.get_mut(&number) {
            episode.title = metadata.get("title").cloned();
            episode.kind = metadata.get("type").cloned();
            episode.air_date = metadata.get("air_date").cloned();
            episode.duration = metadata.get("duration").cloned();
            episode.source = metadata.get("source").cloned();
            episode.audio = metadata.get("audio").cloned();
            episode.softsub = metadata.get("softsub").cloned();
            episode.summary = metadata.get("summary").cloned();
        }

        println!("[{completed}/{total}] {number}: ✓");
    }

    for handle in handles {
        let _ = handle.join();
    }

    save_episodes(&episodes)?;

    println!("\nDone.");

    Ok(())
}