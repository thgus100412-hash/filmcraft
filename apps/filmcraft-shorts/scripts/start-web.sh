#!/usr/bin/env bash
set -euo pipefail
export CARGO_HOME=/workspace/.cargo
export RUSTUP_HOME=/workspace/.rustup
export RUSTUP_TOOLCHAIN=1.95.0
export PATH="$CARGO_HOME/bin:$PATH"
export XDG_DATA_HOME="${XDG_DATA_HOME:-/workspace/.filmcraft-data}"
cd /workspace/filmcraft
cargo build --locked -p filmcraft-shorts --features whisper -j 4
exec ./target/debug/filmcraft-shorts-web --data-dir /workspace/.filmcraft-shorts-web --port 8787
