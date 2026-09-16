#!/usr/bin/env bash
#
# dbz — installer
#
# Builds the Rust TUI and installs it to your PATH so you can run
# `dbz` from anywhere. Also installs the helper binaries.
#
# Usage:
#   ./install.sh            # install to ~/.cargo/bin (default)
#   ./install.sh --prefix ~/.local  # install to ~/.local/bin
#
set -euo pipefail

# Resolve project root (works with symlinks).
SOURCE="${BASH_SOURCE[0]}"
while [ -L "$SOURCE" ]; do
    DIR="$(cd -P "$(dirname "$SOURCE")" >/dev/null 2>&1 && pwd)"
    SOURCE="$(readlink "$SOURCE")"
    [[ $SOURCE != /* ]] && SOURCE="$DIR/$SOURCE"
done
PROJECT_DIR="$(cd -P "$(dirname "$SOURCE")" >/dev/null 2>&1 && pwd)"
RUST_DIR="$PROJECT_DIR/rust"

# Determine install prefix.
PREFIX="${PREFIX:-}"
if [[ "${1:-}" == "--prefix" ]]; then
    PREFIX="$2"
fi

if [[ -z "$PREFIX" ]]; then
    PREFIX="${CARGO_HOME:-$HOME/.cargo}"
fi

INSTALL_DIR="$PREFIX/bin"
mkdir -p "$INSTALL_DIR"

echo "Building release binaries..." >&2
(cd "$RUST_DIR" && cargo build --release) >&2

# Install the main TUI binary.
cp "$RUST_DIR/target/release/dbz" "$INSTALL_DIR/dbz"

# Install the episode data beside the binary so `dbz` works from any directory.
cp "$PROJECT_DIR/episodes.json" "$INSTALL_DIR/episodes.json"

# Install the helper binaries.
for bin in get_video enrich add_covers; do
    if [[ -x "$RUST_DIR/target/release/$bin" ]]; then
        cp "$RUST_DIR/target/release/$bin" "$INSTALL_DIR/$bin"
    fi
done

echo "Installed to $INSTALL_DIR" >&2
echo "Make sure $INSTALL_DIR is on your PATH, then run 'dbz' from anywhere." >&2
echo "Helpers: get_video, enrich, add_covers" >&2
