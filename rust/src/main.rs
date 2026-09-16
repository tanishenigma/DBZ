use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::StatefulImage;

use dbz::{clean_url, data_path, load_episodes, Episode, EpisodeMap};

const HISTORY_FILE: &str = "history.json";

// ============================================================
// Goku theme colors
//   Orange  — his gi
//   Blue    — his undershirt
//   Gold    — his hair
//   Dark    — muted text / secondary
// ============================================================
const GOKU_ORANGE: Color = Color::Rgb(255, 140, 0);
const GOKU_BLUE: Color = Color::Rgb(0, 120, 215);
const GOKU_GOLD: Color = Color::Rgb(255, 200, 0);
const GOKU_DARK: Color = Color::Rgb(150, 150, 150);

/// A single row in the episode list.
struct EpisodeRow {
    number: u32,
    title: String,
    watched: bool,
}

/// Messages sent from the mpv-monitor thread back to the UI thread.
enum MpvMessage {
    Line(String),
    Finished,
}

/// Messages sent from the cover-loading thread back to the UI thread.
enum CoverMessage {
    Loaded(u32, Option<image::DynamicImage>),
}

struct App {
    all_episodes: EpisodeMap,
    filtered: Vec<EpisodeRow>,
    list_state: ListState,
    search: String,
    search_focused: bool,
    current_episode: Option<u32>,
    selected_subtitle: Option<usize>,
    history: BTreeMap<String, serde_json::Value>,
    // Cover image (encoded protocol for the terminal graphics backend).
    picker: Picker,
    cover: Option<Box<dyn StatefulProtocol>>,
    cover_rx: Option<Receiver<CoverMessage>>,
    cover_tx: Option<Sender<CoverMessage>>,
    // Now-playing state
    now_playing: bool,
    np_episode: u32,
    np_title: String,
    np_lines: Vec<String>,
    mpv: Option<Child>,
    mpv_rx: Option<Receiver<MpvMessage>>,
    mpv_tx: Option<Sender<MpvMessage>>,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        let all_episodes = load_episodes().unwrap_or_default();
        let history = load_history();
        // Detect the terminal graphics protocol (Kitty, iTerm2, etc.)
        // and font size. Falls back to halfblocks if unsupported.
        let mut picker = Picker::new((8, 16));
        picker.guess_protocol();
        let mut app = Self {
            all_episodes,
            filtered: Vec::new(),
            list_state: ListState::default(),
            search: String::new(),
            search_focused: false,
            current_episode: None,
            selected_subtitle: None,
            history,
            picker,
            cover: None,
            cover_rx: None,
            cover_tx: None,
            now_playing: false,
            np_episode: 0,
            np_title: String::new(),
            np_lines: Vec::new(),
            mpv: None,
            mpv_rx: None,
            mpv_tx: None,
            should_quit: false,
        };
        app.populate_episodes();
        app.restore_selection();
        app
    }

    fn restore_selection(&mut self) {
        let last_watched = self
            .history
            .get("last_watched")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u32>().ok());

        if let Some(ep) = last_watched {
            if self.all_episodes.contains_key(&ep.to_string()) {
                self.select_episode(ep);
                return;
            }
        }

        self.select_episode(1);
    }

    fn populate_episodes(&mut self) {
        let watched: std::collections::HashSet<String> = self
            .history
            .get("watched")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let query = self.search.trim().to_lowercase();

        // Collect matching episodes, then sort numerically by episode
        // number. The underlying map is keyed by string, which sorts
        // lexicographically ("1", "10", "100", ...), so we sort here.
        let mut matched: Vec<(u32, &Episode)> = self
            .all_episodes
            .iter()
            .filter(|(number, data)| {
                if query.is_empty() {
                    return true;
                }
                let title = data.title.as_deref().unwrap_or("").to_lowercase();
                let summary = data.summary.as_deref().unwrap_or("").to_lowercase();
                query.contains(number.as_str())
                    || title.contains(&query)
                    || summary.contains(&query)
            })
            .filter_map(|(number, data)| number.parse::<u32>().ok().map(|n| (n, data)))
            .collect();

        matched.sort_by_key(|(number, _)| *number);

        self.filtered = matched
            .into_iter()
            .map(|(number, data)| EpisodeRow {
                number,
                title: data
                    .title
                    .clone()
                    .unwrap_or_else(|| format!("Episode {number}")),
                watched: watched.contains(&number.to_string()),
            })
            .collect();

        if self.filtered.is_empty() {
            self.current_episode = None;
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
            let first = self.filtered[0].number;
            self.current_episode = Some(first);
        }

        self.load_cover();
    }

    fn select_episode(&mut self, episode: u32) {
        if !self.all_episodes.contains_key(&episode.to_string()) {
            return;
        }
        for (index, row) in self.filtered.iter().enumerate() {
            if row.number == episode {
                self.list_state.select(Some(index));
                self.current_episode = Some(episode);
                self.selected_subtitle = None;
                self.load_cover();
                return;
            }
        }
    }

    fn next_episode(&mut self) {
        let index = self.list_state.selected().unwrap_or(0);
        if index + 1 >= self.filtered.len() {
            return;
        }
        self.list_state.select(Some(index + 1));
        self.current_episode = Some(self.filtered[index + 1].number);
        self.load_cover();
    }

    fn clear_search(&mut self) {
        self.search.clear();
        self.populate_episodes();
    }

    fn apply_search(&mut self) {
        self.populate_episodes();
    }

    fn current_data(&self) -> Option<&Episode> {
        let ep = self.current_episode?;
        self.all_episodes.get(&ep.to_string())
    }

    /// Load the cover image for the currently selected episode in the
    /// background. The actual disk/network work happens on a worker
    /// thread so the UI stays responsive while scrolling.
    fn load_cover(&mut self) {
        let episode = match self.current_episode {
            Some(ep) => ep,
            None => {
                self.cover = None;
                return;
            }
        };

        let data = match self.all_episodes.get(&episode.to_string()) {
            Some(d) => d.clone(),
            None => {
                self.cover = None;
                return;
            }
        };

        let cover_url = data.cover.clone();

        // Set up the channel if it doesn't exist yet.
        if self.cover_tx.is_none() {
            let (tx, rx) = mpsc::channel();
            self.cover_tx = Some(tx);
            self.cover_rx = Some(rx);
        }
        let tx = self.cover_tx.clone().unwrap();

        thread::spawn(move || {
            // Try to load from cache; if missing, download it.
            let path = dbz::cover_cache_path(episode);
            let path = if path.exists() {
                Some(path)
            } else {
                dbz::download_cover(cover_url.as_deref(), episode)
            };

            let decoded = path.and_then(|p| image::open(&p).ok());
            let _ = tx.send(CoverMessage::Loaded(episode, decoded));
        });
    }

    /// Apply any cover images that finished loading in the background.
    fn handle_cover_messages(&mut self) {
        if let Some(rx) = &self.cover_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    CoverMessage::Loaded(episode, img) => {
                        // Only apply if it still matches the current episode.
                        if self.current_episode == Some(episode) {
                            if let Some(img) = img {
                                // Encode the image for the terminal's
                                // graphics protocol (Kitty, etc.).
                                let protocol = self.picker.new_resize_protocol(img);
                                self.cover = Some(protocol);
                            } else {
                                self.cover = None;
                            }
                        }
                    }
                }
            }
        }
    }

    fn play_episode(&mut self, episode: u32) {
        let data = match self.all_episodes.get(&episode.to_string()) {
            Some(d) => d.clone(),
            None => return,
        };

        let video = data.video.as_deref().map(clean_url).unwrap_or_default();
        if video.is_empty() {
            return;
        }

        let mut command = Command::new("mpv");
        command.arg("--save-position-on-quit").arg(&video);

        // Subtitle selection
        let subtitle = self
            .selected_subtitle
            .and_then(|i| data.subtitles.get(i))
            .or_else(|| data.subtitles.iter().find(|s| s.default && s.file.is_some()));

        if let Some(sub) = subtitle {
            if let Some(file) = sub.file.as_deref() {
                let url = clean_url(file);
                if !url.is_empty() {
                    command.arg(format!("--sub-file={url}"));
                }
            }
        }

        let title = data
            .title
            .clone()
            .unwrap_or_else(|| format!("Episode {episode}"));

        // Set up now-playing state
        self.now_playing = true;
        self.np_episode = episode;
        self.np_title = title;
        self.np_lines.clear();

        // Launch mpv
        let child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match child {
            Ok(child) => {
                let (tx, rx) = mpsc::channel();
                self.mpv_tx = Some(tx);
                self.mpv_rx = Some(rx);
                self.mpv = Some(child);
                self.spawn_mpv_monitor();
            }
            Err(e) => {
                self.np_lines
                    .push(format!("Failed to launch mpv: {e}"));
            }
        }
    }

    /// Record an episode as watched in the history file and persist it.
    fn mark_watched(&mut self, episode: u32) {
        let key = episode.to_string();

        // Add to the watched list (deduplicated).
        let watched = self
            .history
            .entry("watched".to_string())
            .or_insert_with(|| serde_json::Value::Array(vec![]));

        if let serde_json::Value::Array(list) = watched {
            let exists = list
                .iter()
                .any(|v| v.as_str() == Some(key.as_str()));
            if !exists {
                list.push(serde_json::Value::String(key.clone()));
            }
        }

        // Update last_watched.
        self.history.insert(
            "last_watched".to_string(),
            serde_json::Value::String(key),
        );

        save_history(&self.history);
    }

    fn spawn_mpv_monitor(&mut self) {
        let mut child = match self.mpv.take() {
            Some(c) => c,
            None => return,
        };
        let tx = self.mpv_tx.clone().unwrap();

        thread::spawn(move || {
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();

            let mut readers: Vec<Box<dyn BufRead + Send>> = Vec::new();
            if let Some(out) = stdout {
                readers.push(Box::new(BufReader::new(out)));
            }
            if let Some(err) = stderr {
                readers.push(Box::new(BufReader::new(err)));
            }

            let ansi = regex::Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap();

            for reader in readers {
                let mut reader = reader;
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            let cleaned = ansi.replace_all(&line, "").trim().to_string();
                            if !cleaned.is_empty() {
                                let _ = tx.send(MpvMessage::Line(cleaned));
                            }
                        }
                        Err(_) => break,
                    }
                }
            }

            let _ = child.wait();
            let _ = tx.send(MpvMessage::Finished);
        });
    }

    fn handle_mpv_messages(&mut self) {
        // Track whether an episode finished so we can mark it watched
        // after releasing the borrow on the receiver.
        let mut finished_episode: Option<u32> = None;

        if let Some(rx) = &self.mpv_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    MpvMessage::Line(line) => {
                        self.np_lines.push(line);
                        if self.np_lines.len() > 200 {
                            let excess = self.np_lines.len() - 200;
                            self.np_lines.drain(0..excess);
                        }
                    }
                    MpvMessage::Finished => {
                        self.now_playing = false;
                        self.mpv = None;
                        finished_episode = Some(self.np_episode);
                    }
                }
            }
        }

        // Record the finished episode as watched (after the borrow ends).
        if let Some(episode) = finished_episode {
            if episode > 0 {
                self.mark_watched(episode);
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if self.now_playing {
            // While playing, allow q to quit back to the list.
            if key.code == KeyCode::Char('q') {
                self.now_playing = false;
                self.mpv = None;
            }
            return;
        }

        if self.search_focused {
            match key.code {
                KeyCode::Esc => {
                    self.search_focused = false;
                    self.clear_search();
                }
                KeyCode::Enter => {
                    self.search_focused = false;
                }
                KeyCode::Backspace => {
                    self.search.pop();
                    self.apply_search();
                }
                KeyCode::Char(c) => {
                    self.search.push(c);
                    self.apply_search();
                }
                // Allow scrolling the filtered results while searching.
                KeyCode::Up => {
                    let i = self.list_state.selected().unwrap_or(0);
                    if i > 0 {
                        self.list_state.select(Some(i - 1));
                        self.current_episode = Some(self.filtered[i - 1].number);
                        self.load_cover();
                    }
                }
                KeyCode::Down => {
                    let i = self.list_state.selected().unwrap_or(0);
                    if i + 1 < self.filtered.len() {
                        self.list_state.select(Some(i + 1));
                        self.current_episode = Some(self.filtered[i + 1].number);
                        self.load_cover();
                    }
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('n') => self.next_episode(),
            KeyCode::Char('c') => {
                // Subtitle selection is not implemented in the TUI;
                // kept as a no-op to mirror the original.
            }
            KeyCode::Char(c) if c.is_ascii_graphic() || c == ' ' => {
                // Start a search with the typed character.
                self.search_focused = true;
                self.search.clear();
                self.search.push(c);
                self.apply_search();
            }
            KeyCode::Up => {
                let i = self.list_state.selected().unwrap_or(0);
                if i > 0 {
                    self.list_state.select(Some(i - 1));
                    self.current_episode = Some(self.filtered[i - 1].number);
                    self.load_cover();
                }
            }
            KeyCode::Down => {
                let i = self.list_state.selected().unwrap_or(0);
                if i + 1 < self.filtered.len() {
                    self.list_state.select(Some(i + 1));
                    self.current_episode = Some(self.filtered[i + 1].number);
                    self.load_cover();
                }
            }
            KeyCode::Enter => {
                if let Some(ep) = self.current_episode {
                    self.play_episode(ep);
                }
            }
            KeyCode::Esc => {
                self.search_focused = true;
            }
            _ => {}
        }
    }
}

fn load_history() -> BTreeMap<String, serde_json::Value> {
    let path = data_path(HISTORY_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return BTreeMap::new(),
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_history(history: &BTreeMap<String, serde_json::Value>) {
    let path = data_path(HISTORY_FILE);
    if let Ok(text) = serde_json::to_string_pretty(history) {
        let _ = std::fs::write(&path, text);
    }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    let result = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        app.handle_mpv_messages();
        app.handle_cover_messages();

        terminal.draw(|f| ui(f, app))?;

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.on_key(key);
                }
            }
        }
    }
    Ok(())
}

fn ui(f: &mut ratatui::Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(f.area());

    if app.now_playing {
        render_now_playing(f, chunks[0], app);
    } else {
        render_main(f, chunks[0], app);
    }

    render_footer(f, chunks[1], app);
}

fn render_main(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(60), Constraint::Min(0)])
        .split(area);

    render_left(f, chunks[0], app);
    render_right(f, chunks[1], app);
}

fn render_left(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GOKU_ORANGE));

    let inner = block.inner(area);

    let data = app.current_data();

    let title = data
        .and_then(|d| d.title.clone())
        .unwrap_or_else(|| "No episodes found".to_string());

    // Episode name/number header.
    let episode_name = app
        .current_episode
        .map(|ep| format!("Episode {ep:03}"))
        .unwrap_or_default();

    let mut metadata_parts: Vec<String> = Vec::new();
    if let Some(d) = data {
        if let Some(v) = &d.kind {
            metadata_parts.push(v.clone());
        }
        if let Some(v) = &d.air_date {
            metadata_parts.push(v.clone());
        }
        if let Some(v) = &d.duration {
            metadata_parts.push(v.clone());
        }
        if let Some(v) = &d.source {
            metadata_parts.push(v.clone());
        }
        if let Some(v) = &d.audio {
            metadata_parts.push(format!("Audio: {v}"));
        }
        if let Some(v) = &d.softsub {
            metadata_parts.push(format!("CC: {v}"));
        }
    }
    let metadata = metadata_parts.join(" • ");
    let summary = data.and_then(|d| d.summary.clone()).unwrap_or_default();

    // Render the cover image (if available) at the top of the panel.
    let mut cursor_y = inner.y;
    if let Some(protocol) = &mut app.cover {
        // Reserve a fixed area for the image (up to ~40 rows).
        let img_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.min(40),
        };
        let image = StatefulImage::new(None);
        f.render_stateful_widget(image, img_area, protocol);
        cursor_y += img_area.height;
    }

    let lines = vec![
        Line::from(Span::styled(
            episode_name,
            Style::default().fg(GOKU_GOLD).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            title,
            Style::default().fg(GOKU_ORANGE).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            metadata,
            Style::default().fg(GOKU_DARK),
        )),
        Line::from(""),
        Line::from(summary),
    ];

    let paragraph = Paragraph::new(lines)
        .block(Block::default())
        .wrap(Wrap { trim: true });

    // Render the text below the image.
    let text_area = Rect {
        x: inner.x,
        y: cursor_y,
        width: inner.width,
        height: inner.height.saturating_sub(cursor_y - inner.y),
    };
    f.render_widget(paragraph, text_area);

    f.render_widget(block, area);
}

fn render_right(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    // Search input
    let search_block = Block::default()
        .borders(Borders::ALL)
        .title("Search episodes...")
        .border_style(if app.search_focused {
            Style::default().fg(GOKU_GOLD)
        } else {
            Style::default().fg(GOKU_DARK)
        });

    let search_text = if app.search.is_empty() {
        "Type to search...".to_string()
    } else {
        app.search.clone()
    };

    let search_para = Paragraph::new(search_text).block(search_block);
    f.render_widget(search_para, chunks[0]);

    // "Episodes" label
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Episodes",
            Style::default().fg(GOKU_BLUE).add_modifier(Modifier::BOLD),
        ))),
        chunks[1],
    );

    // Episode list
    let items: Vec<ListItem> = app
        .filtered
        .iter()
        .map(|row| {
            let mark = if row.watched { "✓ " } else { "" };
            ListItem::new(format!("{:03}  {}{}", row.number, mark, row.title))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(GOKU_ORANGE)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, chunks[2], &mut app.list_state.clone());
}

fn render_now_playing(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GOKU_ORANGE));

    let title_line = Line::from(Span::styled(
        format!("Now Playing — Episode {:03}", app.np_episode),
        Style::default().fg(GOKU_GOLD).add_modifier(Modifier::BOLD),
    ));

    let subtitle_line = Line::from(Span::styled(
        app.np_title.clone(),
        Style::default().fg(GOKU_DARK),
    ));

    let log_text = app.np_lines.join("\n");

    let mut lines = vec![title_line, subtitle_line, Line::from("")];
    for l in log_text.lines() {
        lines.push(Line::from(l));
    }

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}

fn render_footer(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let hints = if app.now_playing {
        vec!["q Back".to_string()]
    } else {
        vec![
            "q Quit".to_string(),
            "n Next".to_string(),
            "Enter Play".to_string(),
            "Type to search".to_string(),
        ]
    };
    let footer = Paragraph::new(Line::from(Span::styled(
        hints.join("   "),
        Style::default().fg(GOKU_BLUE),
    )));
    f.render_widget(footer, area);
}