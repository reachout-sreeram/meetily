#!/usr/bin/env bash
# Start the Meetily VibeVoice-ASR sidecar on 127.0.0.1 (default port 8765).
# First run creates .venv and installs requirements. Model weights (~5.7 GB)
# download on the first transcription and are cached in ~/.cache/huggingface.
set -euo pipefail
cd "$(dirname "$0")"

if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
  echo "VibeVoice via MLX requires an Apple Silicon Mac (macOS arm64)." >&2
  exit 1
fi
if ! command -v ffmpeg >/dev/null 2>&1; then
  echo "ffmpeg not found. Install it with: brew install ffmpeg" >&2
  exit 1
fi

PYTHON="${PYTHON:-python3}"
if [[ ! -d .venv ]]; then
  echo "Creating virtualenv in $(pwd)/.venv ..."
  "$PYTHON" -m venv .venv
  .venv/bin/pip install --upgrade pip >/dev/null
  .venv/bin/pip install -r requirements.txt
elif [[ "${UPGRADE:-0}" == "1" ]]; then
  .venv/bin/pip install -U -r requirements.txt
fi

exec .venv/bin/python server.py "$@"
