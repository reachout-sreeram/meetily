# VibeVoice-ASR sidecar (post-meeting transcription with speaker labels)

A small local HTTP service that runs Microsoft **VibeVoice-ASR** through
[mlx-audio](https://github.com/Blaizzy/mlx-audio) on Apple Silicon. Meetily uses it
**only** for *Import & Enhance* / re-transcription of a finished recording, not for the
live transcript. It returns segments with **speaker IDs and timestamps**, which Whisper
and Parakeet don't provide.

- Runs fully locally and binds to `127.0.0.1` only. Requests whose `Host` header is not
  localhost are rejected, which blocks DNS-rebinding attacks.
- The model is loaded on first use and **unloaded after 10 idle minutes** to free RAM.
- Unrelated to the archived `backend/` folder.

## Requirements

- Apple Silicon Mac (tested target: M4 Pro, 16 GB). MLX does not run on Intel Macs, Linux or Windows.
- Python 3.10+, `ffmpeg` (`brew install ffmpeg`).
- About 6 GB of disk for `mlx-community/VibeVoice-ASR-4bit`, cached in `~/.cache/huggingface`.

## Run

```bash
cd vibevoice-server
./start.sh                 # first run creates .venv and installs requirements.txt
./start.sh --port 9000     # or VIBEVOICE_PORT=9000 ./start.sh
UPGRADE=1 ./start.sh       # upgrade mlx-audio and the other dependencies
```

The first transcription downloads the weights (~5.7 GB). After that you can enforce
offline operation:

```bash
HF_HUB_OFFLINE=1 ./start.sh
```

In Meetily, open **Settings → Transcription → VibeVoice (local server)**. Check that the URL
(default `http://127.0.0.1:8765`) shows a green "Connected" status, and optionally add hotwords.
Then, on a meeting, choose **Enhance → VibeVoice (local server)**. You can also pick it in
**Import audio**.

## Configuration (environment variables)

| Variable | Default | Meaning |
|---|---|---|
| `VIBEVOICE_PORT` | `8765` | Port on 127.0.0.1 |
| `VIBEVOICE_MODEL` | `mlx-community/VibeVoice-ASR-4bit` | Any mlx-audio VibeVoice-ASR checkpoint |
| `VIBEVOICE_IDLE_MINUTES` | `10` | Unload model after this idle time (`0` = never) |
| `VIBEVOICE_CHUNK_MINUTES` | `25` | Max audio per model pass (see limitations) |
| `VIBEVOICE_OVERLAP_SECONDS` | `30` | Overlap between chunks |
| `VIBEVOICE_MAX_TOKENS` | `32768` | Output-token limit per pass (mlx-audio default is 8192) |
| `VIBEVOICE_LOG_LEVEL` | `info` | `debug` also logs file paths and ffmpeg errors |

At `info` level, logs never contain transcript text or audio paths.

## API

```
GET  /health      {"status":"ok","model":"…","model_loaded":false,"busy":false,"version":"0.1.0","mlx_audio_version":"0.6.4"}
GET  /status      {"active":true,"stage":"transcribing","done":1,"total":3,"message":"Transcribing chunk 2 of 3..."}
POST /transcribe  {"audio_path":"/abs/path/audio.m4a","hotwords":["Acme","Jira"]}
                  or multipart/form-data: file=<audio>, hotwords="Acme, Jira"
  →  {"segments":[{"speaker":"0","start":0.0,"end":3.5,"text":"…"}],
      "language":null,"duration":612.4,"chunks":1,"warnings":[],"processing_seconds":98.1}
POST /unload      free the model now
```

One transcription runs at a time; a second request gets HTTP `409`. Any input that ffmpeg
can read (mp3, mp4, m4a, wav, …) is converted to **24 kHz mono WAV**. That is the model's
native rate, so we skip the lossy 16 kHz → 24 kHz round trip.

## How the model output is handled

- VibeVoice writes a JSON array of `{"Start time", "End time", "Speaker ID", "Content"}`.
  mlx-audio parses it into `STTOutput.segments`, but returns `[]` if the JSON is incomplete.
  This sidecar uses its own tolerant parser (`parsing.py`), which keeps every complete
  segment even when generation stops at the token limit. If that happens, it adds a warning.
- Hotwords are passed through mlx-audio's `hotwords=` argument, which folds them into the
  model's "extra info" prompt. This biases recognition toward those words but does not
  guarantee them.
- The model does not report the language, so `language` is always `null`.

## Known limitations

1. **Long recordings are chunked and speaker IDs do not carry across chunks.** VibeVoice
   accepts up to ~59 minutes per pass, and mlx-audio silently trims anything longer. In
   practice the *output-token limit* is the tighter constraint: a dense hour of speech
   written as JSON can exceed 8k–32k tokens. So the default chunk is **25 minutes** with a
   30 s overlap. Segments in the overlap are assigned to the chunk whose midpoint is
   nearer, so they are not duplicated.
   Each pass assigns speaker IDs independently, so for recordings with more than one chunk
   the IDs are **prefixed with the chunk number** (`1-0`, `2-0`, …). "1-0" and "2-0" may be
   the same person. Give them the same name in Meetily's speaker rename dialog to merge
   them. The response's `warnings` field says so explicitly.
2. Diarization quality depends on the single laptop mic. Overlapping speech and people far
   from the mic are often merged into one speaker.
3. Peak memory on a 16 GB Mac is tight. Close other heavy apps, and lower
   `VIBEVOICE_CHUNK_MINUTES` if you see memory pressure. Run `scripts/verify_vibevoice.py`
   to measure on your machine.

## Step 0 verification

```bash
.venv/bin/python scripts/verify_vibevoice.py ~/meeting_10min.wav --hotwords "Acme,Jira"
```

This runs the CLI (`python -m mlx_audio.stt.generate`) and the Python API, then prints the
raw output, the segment structure, the truncation status, the timing (real-time factor)
and the peak memory.

## Tests

The tests do not need MLX and run on any OS:

```bash
python -m venv .venv-test && .venv-test/bin/pip install -r requirements-dev.txt
.venv-test/bin/python -m pytest tests
```

## License

The sidecar code is MIT, the same as Meetily. VibeVoice-ASR is © Microsoft, MIT license.
mlx-audio is MIT. See `THIRD_PARTY_NOTICES.md` at the repository root.
