# DBZ

A terminal-based Dragon Ball Z episode browser and video player.

## Requirements

- Linux or macOS
- Rust and Cargo
- `mpv` for video playback
- A terminal with true-color support recommended

Install Rust from [rustup.rs](https://rustup.rs/), then install `mpv` using your system package manager.

## Install from GitHub

```bash
git clone https://github.com/tanishenigma/DBZ.git
cd DBZ
./install.sh
```

The installer builds the optimized Rust binary and installs `dbz` in `~/.cargo/bin`. It also installs the episode data beside the binary, so the command can be run from any directory.

Ensure `~/.cargo/bin` is on your `PATH`, then start the app:

```bash
dbz
```

## Update

```bash
cd DBZ
git pull
./install.sh
```

## Uninstall

To remove the default installation manually:

```bash
rm -f "$HOME/.cargo/bin/dbz" "$HOME/.cargo/bin/episodes.json"
```
