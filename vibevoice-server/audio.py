"""Audio conversion with ffmpeg.

VibeVoice-ASR resamples everything to 24 kHz internally, so we convert to
24 kHz mono (not 16 kHz) to avoid a lossy 16k -> 24k round trip.
"""

from __future__ import annotations

import shutil
import subprocess
import wave
from pathlib import Path

import numpy as np

SAMPLE_RATE = 24_000


class AudioError(RuntimeError):
    pass


def ensure_ffmpeg() -> str:
    path = shutil.which("ffmpeg")
    if not path:
        raise AudioError("ffmpeg not found on PATH. Install it with: brew install ffmpeg")
    return path


def convert_to_wav(src: Path, dst: Path, sample_rate: int = SAMPLE_RATE) -> Path:
    """Convert any ffmpeg-readable input (mp3, mp4, m4a, wav, ...) to mono PCM16 WAV."""
    ffmpeg = ensure_ffmpeg()
    cmd = [
        ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error", "-y",
        "-i", str(src),
        "-vn", "-ac", "1", "-ar", str(sample_rate),
        "-c:a", "pcm_s16le", "-f", "wav", str(dst),
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        # stderr can contain the input path; callers log it only at debug level.
        raise AudioError(f"ffmpeg failed (exit {proc.returncode}): {proc.stderr.strip()[-500:]}")
    return dst


def read_wav(path: Path) -> np.ndarray:
    """Read a mono PCM16 WAV produced by ``convert_to_wav`` as float32 in [-1, 1]."""
    with wave.open(str(path), "rb") as wf:
        if wf.getnchannels() != 1 or wf.getsampwidth() != 2:
            raise AudioError("expected mono 16-bit WAV")
        frames = wf.readframes(wf.getnframes())
    return np.frombuffer(frames, dtype="<i2").astype(np.float32) / 32768.0
