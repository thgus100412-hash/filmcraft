#!/usr/bin/env bash
# Native CPU-only local inference, including Intel macOS Monterey. No Homebrew needed.
set -euo pipefail
if [[ "$(uname -s)" != Darwin ]]; then
  echo '이 준비 도구는 맥 전용입니다.' >&2
  exit 1
fi
script_dir="$(cd "$(dirname "$0")" && pwd)"
ai_root="${FILMCRAFT_AI_DIR:-$HOME/Library/Application Support/FilmcraftShorts/ai}"
xcode-select -p >/dev/null
command -v python3 >/dev/null
mkdir -p "$ai_root"
cmake_bin="$ai_root/cmake-3.31.8-macos-universal/CMake.app/Contents/bin/cmake"
if [[ ! -x "$cmake_bin" ]]; then
  archive="$ai_root/cmake-3.31.8-macos-universal.tar.gz"
  if [[ ! -f "$archive" ]]; then
    echo '1/3: 빌드 도구 다운로드 중…'
    curl --proto '=https' --tlsv1.2 -fL --retry 2 https://github.com/Kitware/CMake/releases/download/v3.31.8/cmake-3.31.8-macos-universal.tar.gz -o "$archive.part"
    mv "$archive.part" "$archive"
  fi
  # Official Kitware release SHA-256, verified when this helper was authored.
  printf '%s  %s\n' d1449f969c54d5c00886d5b643340d493dfb3c81cb39ee29b35453395c11ebf7 "$archive" | shasum -a 256 -c -
  tar -xzf "$archive" -C "$ai_root"
fi
source_dir="$ai_root/llama.cpp"
revision=620d2225cc436ab0b001e3200e590e3b286ef9ab
if [[ ! -d "$source_dir" ]]; then
  git clone --depth 1 --branch b11669 https://github.com/ggml-org/llama.cpp.git "$source_dir"
fi
if [[ "$(git -C "$source_dir" rev-parse HEAD)" != "$revision" || -n "$(git -C "$source_dir" status --porcelain)" ]]; then
  echo '기존 AI 실행기 소스가 고정 버전과 다릅니다. 파일을 덮어쓰지 않고 중단했습니다.' >&2
  exit 1
fi
echo '2/3: 맥용 로컬 AI 실행기를 만듭니다. 처음에는 시간이 걸립니다.'
"$cmake_bin" -S "$source_dir" -B "$source_dir/build" -DCMAKE_BUILD_TYPE=Release -DCMAKE_OSX_DEPLOYMENT_TARGET=12.0 -DGGML_METAL=OFF -DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DLLAMA_CURL=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_SERVER=ON
"$cmake_bin" --build "$source_dir/build" --target llama-server --config Release -j 4
"$source_dir/build/bin/llama-server" --version
echo '3/3: 발언 선택용 모델을 내려받고 배포처 SHA-256으로 검사합니다.'
FILMCRAFT_MODEL_DIR="$ai_root/models" python3 "$script_dir/fetch-local-model.py"
echo '준비 완료. 다음 명령을 실행하고 터미널을 켜 두세요:'
echo 'bash apps/filmcraft-shorts/scripts/start-local-model.sh'
