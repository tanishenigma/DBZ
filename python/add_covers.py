import json
import shutil

INPUT_FILE = "episodes.json"
BACKUP_FILE = "episodes_backup.json"


def main():
    # Create a backup before modifying anything
    shutil.copy2(INPUT_FILE, BACKUP_FILE)

    # Load existing JSON
    with open(INPUT_FILE, "r", encoding="utf-8") as f:
        episodes = json.load(f)

    added = 0
    skipped = 0

    for episode_number, episode in episodes.items():

        video_url = episode.get("video")

        if not video_url:
            print(f"[{episode_number}] No video URL - skipped")
            skipped += 1
            continue

        # JSON currently contains escaped slashes like:
        # https:\/\/seiryuu...
        video_url = video_url.replace("\\/", "/")

        # Convert:
        # /UUID/master.m3u8
        #
        # into:
        # /UUID/snapshot.webp
        if not video_url.endswith("/master.m3u8"):
            print(f"[{episode_number}] Unexpected video URL - skipped")
            skipped += 1
            continue

        cover_url = video_url.replace(
            "/master.m3u8",
            "/snapshot.webp"
        )

        episode["cover"] = cover_url

        added += 1

        print(f"[{episode_number}] ✓ {cover_url}")

    # Save modified JSON
    with open(INPUT_FILE, "w", encoding="utf-8") as f:
        json.dump(
            episodes,
            f,
            indent=2,
            ensure_ascii=False
        )

    print()
    print("Done!")
    print(f"Covers added: {added}")
    print(f"Skipped:       {skipped}")
    print(f"Backup:        {BACKUP_FILE}")


if __name__ == "__main__":
    main()