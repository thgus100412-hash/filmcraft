#!/usr/bin/env bash
set -euo pipefail
if [[ "$(uname -s)" == Darwin ]]; then
  ai_root="${FILMCRAFT_AI_DIR:-$HOME/Library/Application Support/FilmcraftShorts/ai}"
  runner="$ai_root/llama.cpp/build/bin/llama-server"
  default_model="$ai_root/models/Qwen3-4B-Instruct-2507-Q4_K_M.gguf"
else
  runner=/workspace/.local-model/llama-b11669/llama-b11669/llama-server
  default_model=/workspace/.local-model/models/Qwen3-4B-Instruct-2507-Q4_K_M.gguf
fi
model="${FILMCRAFT_LOCAL_MODEL:-$default_model}"
if [[ ! -x "$runner" ]]; then
  echo 'AI 실행기가 없습니다. 맥: bash apps/filmcraft-shorts/scripts/setup-mac-ai.sh' >&2
  exit 1
fi
if [[ ! -f "$model" ]]; then
  echo 'AI 모델 파일이 없습니다. 준비 스크립트의 다운로드 오류를 확인하세요.' >&2
  exit 1
fi
echo "로컬 AI를 불러옵니다. 준비가 끝날 때까지 기다린 뒤 웹 화면에서 준비 상태를 다시 확인하세요."
exec "$runner" --model "$model" --alias interview-local --host 127.0.0.1 --port 8000 --threads 4 --ctx-size 32768 --parallel 1 --n-gpu-layers 0 --jinja
