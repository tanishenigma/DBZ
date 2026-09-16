use std::fs;

use anyhow::{Context, Result};

use dbz::{clean_url, load_episodes, save_episodes};

const BACKUP_FILE: &str = "episodes_backup.json";

fn main() -> Result<()> {
    // Create a backup before modifying anything
    fs::copy("episodes.json", BACKUP_FILE)
        .with_context(|| format!("failed to back up to {BACKUP_FILE}"))?;

    let mut episodes = load_episodes()?;

    let mut added = 0;
    let mut skipped = 0;

    for (episode_number, episode) in episodes.iter_mut() {
        let video_url = match &episode.video {
            Some(v) => v.clone(),
            None => {
                println!("[{episode_number}] No video URL - skipped");
                skipped += 1;
                continue;
            }
        };

        // JSON currently contains escaped slashes like:
        // https:\/\/seiryuu...
        let video_url = clean_url(&video_url);

        // Convert:
        // /UUID/master.m3u8
        //
        // into:
        // /UUID/snapshot.webp
        if !video_url.ends_with("/master.m3u8") {
            println!("[{episode_number}] Unexpected video URL - skipped");
            skipped += 1;
            continue;
        }

        let cover_url = video_url.replace("/master.m3u8", "/snapshot.webp");

        episode.cover = Some(cover_url.clone());

        added += 1;

        println!("[{episode_number}] ✓ {cover_url}");
    }

    save_episodes(&episodes)?;

    println!();
    println!("Done!");
    println!("Covers added: {added}");
    println!("Skipped:       {skipped}");
    println!("Backup:        {BACKUP_FILE}");

    Ok(())
}