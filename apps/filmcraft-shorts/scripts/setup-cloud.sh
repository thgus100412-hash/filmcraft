#!/usr/bin/env bash
set -euo pipefail
export CARGO_HOME=/workspace/.cargo
export RUSTUP_HOME=/workspace/.rustup
export RUSTUP_TOOLCHAIN=1.95.0
export PATH="$CARGO_HOME/bin:$PATH"
export XDG_DATA_HOME="${XDG_DATA_HOME:-/workspace/.filmcraft-data}"
mkdir -p "$CARGO_HOME" "$RUSTUP_HOME" "$XDG_DATA_HOME" /workspace/.local-model
if [[ ! -x "$CARGO_HOME/bin/rustup" ]]; then
  installer=$(mktemp /tmp/filmcraft-rustup.XXXXXX)
  trap 'rm -f "$installer"' EXIT
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$installer"
  sh "$installer" -y --no-modify-path --profile minimal --default-toolchain 1.95.0
fi
rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy
archive=/workspace/.local-model/llama-b11669.tar.gz
if [[ ! -f "$archive" ]]; then
  curl --proto '=https' --tlsv1.2 -fsSL https://github.com/ggml-org/llama.cpp/releases/download/b11669/llama-b11669-bin-ubuntu-x64.tar.gz -o "$archive"
fi
# Official GitHub release asset digest, verified during onboarding.
echo 'cf07c1c112d709bd67e522017471de57ed0ec8bea031d732c5422351c07f65ac  /workspace/.local-model/llama-b11669.tar.gz' | sha256sum --check --status
if [[ ! -x /workspace/.local-model/llama-b11669/llama-b11669/llama-server ]]; then
  python3 - <<'PY'
import tarfile
with tarfile.open('/workspace/.local-model/llama-b11669.tar.gz') as archive:
    archive.extractall('/workspace/.local-model/llama-b11669', filter='data')
PY
fi
/workspace/.local-model/llama-b11669/llama-b11669/llama-server --version
cd /workspace/filmcraft
cargo build --locked -p filmcraft-shorts --features whisper -j 4
./target/debug/filmcraft-shorts --help
# Model weights are a separate prerequisite: see fetch-local-model.py and README.
