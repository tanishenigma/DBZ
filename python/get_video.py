import json
import sys
import time

import requests
from bs4 import BeautifulSoup

BASE_URL = "https://anizone.to/anime/99nxtlfu/{}"
START_EPISODE = 1
END_EPISODE = 291

HEADERS = {
    "User-Agent": "Mozilla/5.0"
}


def get_video_data(page_url):
    response = requests.get(
        page_url,
        headers=HEADERS,
        timeout=15
    )
    response.raise_for_status()

    soup = BeautifulSoup(response.text, "html.parser")

    player = soup.find(
        attrs={
            "x-data": lambda value:
                value and "vidstackPlayer" in value
        }
    )

    if not player:
        raise RuntimeError("Video configuration not found")

    x_data = player["x-data"]

    start = x_data.find("JSON.parse('")

    if start == -1:
        raise RuntimeError("JSON.parse data not found")

    start += len("JSON.parse('")

    end = x_data.find("')", start)

    if end == -1:
        raise RuntimeError("End of JSON data not found")

    encoded_json = x_data[start:end]

    decoded_json = encoded_json.replace("\\/", "/")
    decoded_json = decoded_json.replace("\\u0022", '"')

    return json.loads(decoded_json)


def main():
    results = {}

    for episode in range(START_EPISODE, END_EPISODE + 1):

        url = BASE_URL.format(episode)

        print(f"[{episode}/{END_EPISODE}] Fetching episode {episode}...")

        try:
            data = get_video_data(url)

            results[str(episode)] = {
                "page": url,
                "video": data.get("src"),
                "subtitles": data.get("subtitles", [])
            }

            print("  ✓ Found video")
            print(f"  ✓ Subtitles: {len(data.get('subtitles', []))}")

        except Exception as e:
            print(f"  ✗ Error: {e}")

            results[str(episode)] = {
                "page": url,
                "error": str(e)
            }

        # Small delay between requests
        time.sleep(0.5)

    with open("episodes.json", "w", encoding="utf-8") as f:
        json.dump(
            results,
            f,
            indent=2,
            ensure_ascii=False
        )

    print("\nDone!")
    print("Saved to episodes.json")


if __name__ == "__main__":
    main()