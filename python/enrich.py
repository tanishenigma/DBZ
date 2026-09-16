import json
import re
from concurrent.futures import ThreadPoolExecutor, as_completed

import requests
from bs4 import BeautifulSoup

INPUT_FILE = "episodes.json"

HEADERS = {
    "User-Agent": "Mozilla/5.0"
}


def extract_metadata(page_url):

    response = requests.get(
        page_url,
        headers=HEADERS,
        timeout=15
    )

    response.raise_for_status()

    soup = BeautifulSoup(
        response.text,
        "html.parser"
    )

    data = {}

    # -----------------------------------------
    # EPISODE HEADING
    # -----------------------------------------

    heading = soup.find("h1")

    if heading:

        title = heading.get_text(
            " ",
            strip=True
        )

        # AniZone currently gives:
        # "- Episode 1"
        data["title"] = title.lstrip("- ").strip()

    # -----------------------------------------
    # FIND MAIN EPISODE INFORMATION
    # -----------------------------------------

    # Find the h1 and use the following
    # content rather than random <p> tags.
    #
    # This prevents the site's ad message
    # from becoming the summary.

    if heading:

        parent = heading.parent

        if parent:

            text = parent.get_text(
                "\n",
                strip=True
            )

            lines = [
                line.strip()
                for line in text.splitlines()
                if line.strip()
            ]

            # ---------------------------------
            # TYPE
            # ---------------------------------

            for value in (
                "Regular",
                "Special"
            ):

                if value in lines:

                    data["type"] = value
                    break

            # ---------------------------------
            # DATE
            # ---------------------------------

            for line in lines:

                match = re.search(
                    r"\d{4}-\d{2}-\d{2}",
                    line
                )

                if match:

                    data["air_date"] = (
                        match.group(0)
                    )

                    break

    # -----------------------------------------
    # SUMMARY
    # -----------------------------------------

    # AniZone's actual episode summary is the
    # paragraph after the episode information.
    #
    # We specifically reject the site's
    # ad-blocker message.

    paragraphs = soup.find_all("p")

    for paragraph in paragraphs:

        summary = paragraph.get_text(
            " ",
            strip=True
        )

        if not summary:
            continue

        if (
            "intrusive ads" in
            summary.lower()
        ):
            continue

        if (
            "disable your adblock" in
            summary.lower()
        ):
            continue

        if len(summary) >= 40:

            data["summary"] = summary
            break

    # -----------------------------------------
    # PAGE TEXT
    # -----------------------------------------

    lines = [
        line.strip()
        for line in soup.get_text(
            "\n",
            strip=True
        ).splitlines()
        if line.strip()
    ]

    # -----------------------------------------
    # DURATION
    # -----------------------------------------

    for i, line in enumerate(lines):

        if line == "Duration:":

            if i + 1 < len(lines):

                value = lines[i + 1]

                if re.match(
                    r"^\d+:\d+$",
                    value
                ):

                    data["duration"] = value

    # -----------------------------------------
    # SOURCE
    # -----------------------------------------

    for i, line in enumerate(lines):

        if line == "Source:":

            if i + 1 < len(lines):

                data["source"] = (
                    lines[i + 1]
                )

    # -----------------------------------------
    # AUDIO
    # -----------------------------------------

    for i, line in enumerate(lines):

        if line == "Audio:":

            if i + 1 < len(lines):

                data["audio"] = (
                    lines[i + 1]
                )

    # -----------------------------------------
    # SOFTSUB
    # -----------------------------------------

    for i, line in enumerate(lines):

        if line == "Softsub:":

            if i + 1 < len(lines):

                data["softsub"] = (
                    lines[i + 1]
                )

    return data


def fetch_episode(item):

    episode, episode_data = item

    try:

        metadata = extract_metadata(
            episode_data["page"]
        )

        return episode, metadata, None

    except Exception as error:

        return episode, {}, str(error)


def main():

    with open(
        INPUT_FILE,
        "r",
        encoding="utf-8"
    ) as file:

        episodes = json.load(file)

    total = len(episodes)

    print(
        f"Fetching {total} episodes..."
    )

    # 20 simultaneous requests.
    # Much faster than sequential fetching.

    with ThreadPoolExecutor(
        max_workers=20
    ) as executor:

        futures = [
            executor.submit(
                fetch_episode,
                item
            )
            for item in episodes.items()
        ]

        completed = 0

        for future in as_completed(futures):

            episode, metadata, error = (
                future.result()
            )

            completed += 1

            if error:

                print(
                    f"[{completed}/{total}] "
                    f"{episode}: ERROR {error}"
                )

                continue

            episodes[episode].update(
                metadata
            )

            print(
                f"[{completed}/{total}] "
                f"{episode}: ✓"
            )

    with open(
        INPUT_FILE,
        "w",
        encoding="utf-8"
    ) as file:

        json.dump(
            episodes,
            file,
            indent=2,
            ensure_ascii=False
        )

    print("\nDone.")


if __name__ == "__main__":
    main()