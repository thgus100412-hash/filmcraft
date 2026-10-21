#!/usr/bin/env bash
set -euo pipefail
runner=/workspace/.local-model/llama-b11669/llama-b11669/llama-server
model=${FILMCRAFT_LOCAL_MODEL:-/workspace/.local-model/models/Qwen3-4B-Instruct-2507-Q4_K_M.gguf}
if [[ ! -x "$runner" ]]; then
  echo 'Local inference runtime is missing; run the environment installation script.' >&2
  exit 1
fi
if [[ ! -f "$model" ]]; then
  echo 'Local model weights are missing. Enable the saved model download domains, then run scripts/fetch-local-model.py.' >&2
  exit 1
fi
exec "$runner" --model "$model" --alias interview-local --host 127.0.0.1 --port 8000 --threads 4 --ctx-size 32768 --parallel 1 --n-gpu-layers 0 --jinja
