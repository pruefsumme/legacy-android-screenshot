#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo is required; install Rust from https://rustup.rs/" >&2
    exit 1
fi

prefix="${PREFIX:-$HOME/.local}"
bin_dir="${BIN_DIR:-$prefix/bin}"
binary="legacy-android-screenshot"

echo "Building ${binary}..."
cargo build --release --locked

install -Dm755 "target/release/${binary}" "${bin_dir}/${binary}"
echo "Installed ${binary} to ${bin_dir}/${binary}"
