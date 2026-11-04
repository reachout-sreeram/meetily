#!/usr/bin/env python3
"""Run the real sidecar HTTP layer with a FAKE model, for UI/integration testing on any OS.

    python scripts/dev_fake_server.py --port 8765

Audio is still converted with ffmpeg (so bad files fail like in production), but the
"transcript" is canned multi-speaker text. Never use this for real meetings.
"""

import argparse
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import uvicorn  # noqa: E402

import server  # noqa: E402
from audio import SAMPLE_RATE  # noqa: E402

LINES = [
    ("0", "Morning everyone, let's go through the Acme rollout."),
    ("1", "The migration finished yesterday. I still need to update the runbook."),
    ("2", "Customer feedback was positive. I'll send the summary to their team by Friday."),
    ("0", "Great. Sam owns the runbook, Priya owns the customer follow-up."),
]


class FakeEngine:
    model_id = "fake/vibevoice"
    loaded = False
    busy = False

    def __init__(self, delay: float):
        self.delay = delay

    def transcribe(self, samples, hotwords, progress):
        duration = len(samples) / SAMPLE_RATE
        self.busy = True
        try:
            progress("loading", 0, 1, "Loading VibeVoice model...")
            time.sleep(self.delay / 2)
            progress("transcribing", 0, 1, "Transcribing chunk 1 of 1...")
            time.sleep(self.delay / 2)
            step = max(duration, 1.0) / len(LINES)
            segs = [
                {"speaker": spk, "start": round(i * step, 2), "end": round((i + 1) * step - 0.1, 2), "text": text}
                for i, (spk, text) in enumerate(LINES)
            ]
            self.loaded = True
            return {"segments": segs, "language": None, "duration": round(duration, 3), "chunks": 1,
                    "warnings": [] if duration < 3600 else ["fake chunk warning"]}
        finally:
            self.busy = False

    def unload(self):
        self.loaded = False

    def shutdown(self):
        pass


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--delay", type=float, default=3.0, help="simulated inference seconds")
    args = ap.parse_args()
    uvicorn.run(server.create_app(FakeEngine(args.delay)), host="127.0.0.1", port=args.port)
