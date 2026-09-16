# dbz — Makefile
#
# Convenience targets for building, running, and installing the app.

RUST_DIR := rust

.PHONY: build run install uninstall clean

## Build the Rust binaries (debug)
build:
	cd $(RUST_DIR) && cargo build

## Build release binaries
release:
	cd $(RUST_DIR) && cargo build --release

## Run the TUI app
run:
	cd $(RUST_DIR) && cargo run --bin dbz

## Install to ~/.cargo/bin (or use PREFIX=~/.local)
install:
	./install.sh

## Uninstall from ~/.cargo/bin
uninstall:
	rm -f "$${CARGO_HOME:-$$HOME/.cargo}/bin/dbz" \
	      "$${CARGO_HOME:-$$HOME/.cargo}/bin/get_video" \
	      "$${CARGO_HOME:-$$HOME/.cargo}/bin/enrich" \
	      "$${CARGO_HOME:-$$HOME/.cargo}/bin/add_covers"

## Clean build artifacts
clean:
	cd $(RUST_DIR) && cargo clean