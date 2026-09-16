use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const EPISODES_FILE: &str = "episodes.json";
pub const CACHE_DIR: &str = "cache/covers";

/// A single episode's metadata as stored in `episodes.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Episode {
    pub page: Option<String>,
    pub video: Option<String>,
    pub subtitles: Vec<Subtitle>,
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub air_date: Option<String>,
    pub duration: Option<String>,
    pub source: Option<String>,
    pub audio: Option<String>,
    pub softsub: Option<String>,
    pub summary: Option<String>,
    pub cover: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Subtitle {
    pub file: Option<String>,
    #[serde(default)]
    pub default: bool,
    pub label: Option<String>,
}

/// All episodes keyed by episode number (as a string, matching the JSON).
pub type EpisodeMap = BTreeMap<String, Episode>;

/// Resolve the project root directory.
///
/// The app stores its data files (`episodes.json`, `history.json`,
/// `cache/`) next to the compiled binary. This lets the app run from
/// any working directory.
///
/// Resolution order:
///   1. The current working directory (if it contains the data).
///   2. Walk up from the executable looking for the data file.
///   3. The current working directory as a last resort.
pub fn data_dir() -> PathBuf {
    // 1. Current working directory first — this makes the installed
    //    binary work when run from the project directory.
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join(EPISODES_FILE).exists() {
            return cwd;
        }
    }

    // 2. Walk up from the executable looking for the data file.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(mut dir) = exe.parent() {
            loop {
                if dir.join(EPISODES_FILE).exists() {
                    return dir.to_path_buf();
                }
                match dir.parent() {
                    Some(parent) => dir = parent,
                    None => break,
                }
            }
        }
    }

    // 3. Fall back to the current working directory.
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Absolute path to a data file inside the project data directory.
pub fn data_path(name: &str) -> PathBuf {
    data_dir().join(name)
}

/// Load the episodes JSON file.
pub fn load_episodes() -> Result<EpisodeMap> {
    let path = data_path(EPISODES_FILE);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let map = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(map)
}

/// Save the episodes JSON file.
pub fn save_episodes(episodes: &EpisodeMap) -> Result<()> {
    let path = data_path(EPISODES_FILE);
    let text = serde_json::to_string_pretty(episodes)?;
    std::fs::write(&path, text)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// The JSON can contain escaped slashes like `https:\\/\\/...`.
/// Repeatedly collapse `\/` into `/` until none remain.
pub fn clean_url(url: &str) -> String {
    let mut out = url.to_string();
    while out.contains("\\/") {
        out = out.replace("\\/", "/");
    }
    out
}

/// Path to the cached cover for an episode (zero-padded to 3 digits).
pub fn cover_cache_path(episode: u32) -> PathBuf {
    data_dir().join(CACHE_DIR).join(format!("{episode:03}.webp"))
}

/// Download a cover image into the cache, returning the local path.
/// Returns `None` if there is no URL or the download fails.
pub fn download_cover(url: Option<&str>, episode: u32) -> Option<PathBuf> {
    let url = url?;
    let url = clean_url(url);

    let path = cover_cache_path(episode);

    if path.exists() {
        return Some(path);
    }

    let response = reqwest::blocking::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .ok()?;

    let bytes = response.bytes().ok()?;

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    std::fs::write(&path, bytes).ok()?;

    Some(path)
}