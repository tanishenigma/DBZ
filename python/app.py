import json
import re
import subprocess
from pathlib import Path

import requests
from textual.app import App, ComposeResult
from textual.containers import Horizontal, Vertical
from textual.widgets import (
    Footer,
    Input,
    Label,
    ListItem,
    ListView,
    Static,
)
from textual_image.widget import TGPImage

EPISODES_FILE = "episodes.json"
CACHE_DIR = Path("cache/covers")


def clean_url(url):
    if not url:
        return None

    # Your JSON can contain:
    # https:\\/\\/...
    #
    # First pass turns:
    # \\/ -> \/
    #
    # Second pass turns:
    # \/ -> /

    while "\\/" in url:
        url = url.replace("\\/", "/")

    return url


def download_cover(url, episode):

    url = clean_url(url)

    if not url:
        return None

    CACHE_DIR.mkdir(
        parents=True,
        exist_ok=True
    )

    path = CACHE_DIR / f"{episode:03d}.webp"

    if path.exists():
        return path

    response = requests.get(
        url,
        timeout=10,
        headers={
            "User-Agent": "Mozilla/5.0"
        }
    )

    response.raise_for_status()

    path.write_bytes(
        response.content
    )

    return path


class EpisodeItem(ListItem):

    def __init__(
        self,
        episode_number,
        data,
        watched=False
    ):
        super().__init__()

        self.episode_number = episode_number
        self.data = data
        self.watched = watched

    def compose(self):

        title = self.data.get(
            "title",
            f"Episode {self.episode_number:03d}"
        )

        mark = "✓ " if self.watched else ""

        yield Label(
            f"{self.episode_number:03d}  {mark}{title}"
        )


class NowPlaying(Static):

    def __init__(self, episode, title):
        super().__init__(id="now-playing")
        self.episode = episode
        self.title = title
        self.lines = []

    def compose(self):

        yield Label(
            f"Now Playing — Episode {self.episode:03d}",
            id="np-title"
        )

        yield Label(
            self.title,
            id="np-subtitle"
        )

        yield Static(
            "",
            id="np-log"
        )

    def append_line(self, line):

        self.lines.append(line)

        # Keep only the last 200 lines.
        self.lines = self.lines[-200:]

        self.query_one(
            "#np-log",
            Static
        ).update(
            "\n".join(self.lines)
        )


class AnimeTUI(App):

    ENABLE_COMMAND_PALETTE = False

    CSS = """

    Screen {
        layout: vertical;
    }

    #main {
        height: 1fr;
    }

    #left {
        width: 35;
        padding: 1 2;
        border: solid $accent;
        overflow: hidden;
    }

    #right {
        width: 1fr;
        padding: 1 2;
    }

    #cover {
        width: auto;
        height: auto;
        margin-bottom: 1;
    }

    #anime-title {
        text-style: bold;
        padding-bottom: 1;
    }

    #metadata {
        color: $text-muted;
    }

    #summary {
        padding-top: 1;
        height: auto;
    }

    #search {
        display: block;
        margin-bottom: 1;
    }

    #episodes {
        height: 1fr;
        border: solid $accent;
    }

    #episodes.hidden {
        display: none;
    }

    /*
     * By default Textual only shows a strong highlight color on the
     * ListView's currently-selected item while the ListView itself has
     * keyboard focus. In this app focus jumps to the search Input the
     * moment you press a letter key, so the "selected" episode would
     * fade to a barely-visible muted highlight and look unselected.
     * These two rules force the same bold, visible highlight
     * regardless of which widget currently has focus.
     */

    #episodes > ListItem.-highlight {
        background: $accent;
        color: $text;
        text-style: bold;
        border-left: thick $accent-lighten-2;
    }

    #episodes:focus > ListItem.-highlight {
        background: $accent;
        color: $text;
        text-style: bold;
        border-left: thick $accent-lighten-2;
    }

    #now-playing {
        display: none;
        height: 1fr;
        padding: 1 2;
        border: solid $accent;
    }

    #now-playing.visible {
        display: block;
    }

    #np-title {
        text-style: bold;
        padding-bottom: 1;
    }

    #np-subtitle {
        color: $text-muted;
        padding-bottom: 1;
    }

    #np-log {
        height: 1fr;
        overflow-y: auto;
        color: $text-muted;
    }

    """

    BINDINGS = [
        ("q", "quit", "Quit"),
        ("escape", "clear_search", "Clear search"),
    ]

    def __init__(self):

        super().__init__()

        with open(
            EPISODES_FILE,
            "r",
            encoding="utf-8"
        ) as file:

            self.all_episodes = json.load(file)

        self.filtered_episodes = (
            self.all_episodes.copy()
        )

        self.history = self.load_history()

        self.selected_subtitle = None
        self.current_episode = None

        self.cover_widget = None
        self.episodes_list = None
        self.now_playing = None

        self.mpv_process = None
        self.search_timer = None

    def load_history(self):

        path = Path("history.json")

        if not path.exists():
            return {}

        try:

            with open(
                path,
                "r",
                encoding="utf-8"
            ) as file:

                return json.load(file)

        except (json.JSONDecodeError, OSError):

            return {}

    def compose(self):

        with Horizontal(id="main"):

            # ==================================
            # LEFT
            # ==================================

            with Vertical(id="left"):

                self.cover_widget = TGPImage(
                    id="cover"
                )

                yield self.cover_widget

                yield Label(
                    "Dragon Ball Z",
                    id="anime-title"
                )

                yield Label(
                    "291 Episodes",
                    id="metadata"
                )

                yield Static(
                    "",
                    id="summary"
                )

            # ==================================
            # RIGHT
            # ==================================

            with Vertical(id="right"):

                self.search_input = Input(
                    placeholder=(
                        "Search episodes..."
                    ),
                    id="search"
                )

                yield self.search_input

                yield Label(
                    "Episodes"
                )

                self.episodes_list = ListView(
                    id="episodes"
                )

                yield self.episodes_list

                self.now_playing = NowPlaying(
                    0,
                    ""
                )

                yield self.now_playing

        yield Footer()

    def on_mount(self):

        self.populate_episodes()

        # Automatically focus the episode list.
        self.episodes_list.focus()

        # Restore previously watched episode.
        last_watched = self.history.get(
            "last_watched"
        )

        if last_watched is not None:

            try:
                last_watched = int(last_watched)

                if str(last_watched) in self.all_episodes:
                    self.select_episode(last_watched)
                    return

            except (ValueError, TypeError):
                pass

        # No history -> start at episode 1.
        self.select_episode(1)

    # ==========================================
    # EPISODE LIST
    # ==========================================

    def populate_episodes(self):

        self.episodes_list.clear()

        watched = set(
            self.history.get(
                "watched",
                []
            )
        )

        for number, data in (
            self.filtered_episodes.items()
        ):

            self.episodes_list.append(
                EpisodeItem(
                    int(number),
                    data,
                    str(number) in watched
                )
            )

 
        if self.episodes_list.children:

            self.episodes_list.index = 0

            first_item = self.episodes_list.children[0]

            self.current_episode = first_item.episode_number

            self.sync_details(
                self.current_episode
            )

        else:

            self.current_episode = None

            self.clear_details()

    def select_episode(self, episode):

        # Make sure the episode exists.
        if str(episode) not in self.filtered_episodes:
            return

        for index, item in enumerate(
            self.episodes_list.children
        ):

            if item.episode_number == episode:

                self.episodes_list.index = index

                self.current_episode = episode

                # Reset manual subtitle selection.
                self.selected_subtitle = None

                self.sync_details(
                    episode
                )

                self.episodes_list.focus()

                return

    # ==========================================
    # HIGHLIGHT
    # ==========================================

    def on_list_view_highlighted(
        self,
        event: ListView.Highlighted
    ):

        if event.item is None:
            return

        episode = (
            event.item.episode_number
        )

        self.current_episode = episode

        self.sync_details(
            episode
        )

    # ==========================================
    # DETAILS
    # ==========================================

    def sync_details(self, episode):
        """Single place that keeps every piece of UI driven by the
        currently selected/highlighted episode in sync. Called from
        every code path that changes which episode is selected, so
        selection always visibly "controls" the title, metadata,
        summary and cover — even right after a search/filter."""

        self.update_episode_info(
            episode
        )

        self.update_cover(
            episode
        )

    def clear_details(self):

        self.query_one(
            "#anime-title",
            Label
        ).update(
            "No episodes found"
        )

        self.query_one(
            "#metadata",
            Label
        ).update(
            ""
        )

        self.query_one(
            "#summary",
            Static
        ).update(
            ""
        )

    def update_episode_info(
        self,
        episode
    ):

        data = self.all_episodes.get(
            str(episode)
        )

        if not data:
            return

        title = data.get(
            "title",
            f"Episode {episode}"
        )

        self.query_one(
            "#anime-title",
            Label
        ).update(title)

        metadata = []

        if data.get("type"):
            metadata.append(
                data["type"]
            )

        if data.get("air_date"):
            metadata.append(
                data["air_date"]
            )

        if data.get("duration"):
            metadata.append(
                data["duration"]
            )

        if data.get("source"):
            metadata.append(
                data["source"]
            )

        if data.get("audio"):
            metadata.append(
                f"Audio: {data['audio']}"
            )

        if data.get("softsub"):
            metadata.append(
                f"CC: {data['softsub']}"
            )

        self.query_one(
            "#metadata",
            Label
        ).update(
            " • ".join(metadata)
        )

        self.query_one(
            "#summary",
            Static
        ).update(
            data.get(
                "summary",
                ""
            )
        )

    # ==========================================
    # COVER
    # ==========================================

    def update_cover(self, episode):

        data = self.all_episodes.get(
            str(episode)
        )

        if not data:
            return

        cover = data.get(
            "cover"
        )

        if not cover:
            return

        cached = (
            CACHE_DIR /
            f"{episode:03d}.webp"
        )

        # ----------------------------------
        # Already cached = instant
        # ----------------------------------

        if cached.exists():

            self.cover_widget.image = str(
                cached
            )

            return

        # ----------------------------------
        # Download in background
        # ----------------------------------

        self.run_worker(
            lambda: self.load_cover(
                episode
            ),
            thread=True,
            exclusive=True
        )

    def load_cover(self, episode):

        data = self.all_episodes.get(
            str(episode)
        )

        if not data:
            return

        try:

            path = download_cover(
                data.get("cover"),
                episode
            )

            self.call_from_thread(
                self.set_cover,
                episode,
                path
            )

        except Exception as error:

            print(
                f"Cover error: {error}"
            )

    def set_cover(
        self,
        episode,
        path
    ):

        # Only apply the cover if it still matches whatever episode is
        # currently selected — avoids a slow download for an episode
        # the user has already navigated away from clobbering the
        # cover that's actually on screen.
        if self.current_episode != episode:
            return

        self.cover_widget.image = str(
            path
        )

    # ==========================================
    # SEARCH
    # ==========================================

    def on_input_changed(
        self,
        event: Input.Changed
    ):

        # Debounce: wait until the user stops typing.
        self.set_timer(
            0.3,
            lambda: self.apply_search(
                event.value
            )
        )

    def apply_search(self, value):

        query = value.lower().strip()

        if not query:

            self.filtered_episodes = (
                self.all_episodes.copy()
            )

        else:

            self.filtered_episodes = {
                number: data
                for number, data
                in self.all_episodes.items()
                if (
                    query in str(number)
                    or query in data.get(
                        "title",
                        ""
                    ).lower()
                    or query in data.get(
                        "summary",
                        ""
                    ).lower()
                )
            }

        self.populate_episodes()

    def action_clear_search(self):

        search = self.query_one(
            "#search",
            Input
        )

        search.value = ""

        self.filtered_episodes = (
            self.all_episodes.copy()
        )

        self.populate_episodes()

        self.episodes_list.focus()

    # ==========================================
    # PLAY
    # ==========================================

    def on_list_view_selected(
        self,
        event: ListView.Selected
    ):

        self.play_episode(
            event.item.episode_number
        )

    def play_episode(self, episode):

        data = self.all_episodes[
            str(episode)
        ]

        video = clean_url(
            data.get("video")
        )

        if not video:
            return

        command = [
            "mpv",
            "--save-position-on-quit",
            video
        ]

        # ----------------------------------
        # Subtitle
        # ----------------------------------

        subtitle = self.selected_subtitle

        # If the user hasn't manually selected one,
        # use the JSON default subtitle.

        if subtitle is None:

            subtitles = data.get(
                "subtitles",
                []
            )

            subtitle = next(
                (
                    item
                    for item in subtitles
                    if item.get("default") is True
                    and item.get("file")
                ),
                None
            )

        if subtitle:

            subtitle_url = clean_url(
                subtitle.get("file")
            )

            if subtitle_url:

                command.append(
                    f"--sub-file={subtitle_url}"
                )

        # ----------------------------------
        # Now Playing screen
        # ----------------------------------

        title = data.get(
            "title",
            f"Episode {episode}"
        )

        self.now_playing.episode = episode
        self.now_playing.title = title

        self.now_playing.query_one(
            "#np-title",
            Label
        ).update(
            f"Now Playing — Episode {episode:03d}"
        )

        self.now_playing.query_one(
            "#np-subtitle",
            Label
        ).update(title)

        self.now_playing.lines = []

        self.now_playing.query_one(
            "#np-log",
            Static
        ).update("")

        self.episodes_list.add_class(
            "hidden"
        )

        self.now_playing.add_class(
            "visible"
        )

        # ----------------------------------
        # Launch mpv
        # ----------------------------------

        self.mpv_process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1
        )

        self.run_worker(
            self._monitor_mpv,
            thread=True,
            exclusive=True
        )

    def _monitor_mpv(self):

        process = self.mpv_process

        if process is None:
            return

        ansi = re.compile(
            r"\x1b\[[0-9;]*[A-Za-z]"
        )

        for line in process.stdout:

            line = ansi.sub("", line).rstrip()

            if not line:
                continue

            self.call_from_thread(
                self.now_playing.append_line,
                line
            )

        process.wait()

        self.call_from_thread(
            self._mpv_finished
        )

    def _mpv_finished(self):

        self.mpv_process = None

        self.now_playing.remove_class(
            "visible"
        )

        self.episodes_list.remove_class(
            "hidden"
        )

        self.episodes_list.focus()

        # Re-sync the left panel with whatever is highlighted now
        # that the episode list is visible again.
        if self.current_episode is not None:

            self.sync_details(
                self.current_episode
            )

    # ==========================================
    # KEYBOARD
    # ==========================================

    def on_key(self, event):

        # Search box already active:
        # let it receive all keyboard input.
        if self.search_input.has_focus:
            return

        char = event.character

        if not char:
            return

        if not char.isprintable():
            return

        # -------------------------------
        # Shortcuts
        # -------------------------------

        if char == "c":

            self.action_subtitle()

            event.stop()

            return

        if char == "n":

            self.action_next_episode()

            event.stop()

            return

        if char == "q":

            self.action_quit()

            event.stop()

            return

        # -------------------------------
        # Start search
        # -------------------------------

        self.search_input.focus()

        self.search_input.value = char

        event.stop()

    def action_next_episode(self):

        if not self.episodes_list.children:
            return

        index = self.episodes_list.index

        if index is None:
            index = 0

        if index + 1 >= len(
            self.episodes_list.children
        ):
            return

        self.episodes_list.index = index + 1

        self._sync_from_index(
            index + 1
        )

        self.episodes_list.focus()

    def _sync_from_index(self, index):
        """Used by the n/p shortcuts: immediately drives the details
        panel from whatever item now lives at `index`, rather than
        waiting for the Highlighted message to be processed."""

        item = self.episodes_list.children[index]

        self.current_episode = item.episode_number

        self.sync_details(
            self.current_episode
        )

    def action_subtitle(self):

        pass

    def action_quit(self):

        self.exit()


if __name__ == "__main__":

    AnimeTUI().run()