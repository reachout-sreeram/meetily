"""Local VibeVoice-ASR sidecar for Meetily.

Binds to 127.0.0.1 only. No cloud calls: the model is downloaded once from
Hugging Face on first load and cached locally (set HF_HUB_OFFLINE=1 afterwards
to forbid any network access).

Endpoints
---------
GET  /health      -> {"status", "model", "model_loaded", "busy", "version", "mlx_audio_version"}
GET  /status      -> progress of the running job (polled by the app)
POST /transcribe  -> JSON {"audio_path": str, "hotwords": [str]}  or
                     multipart form with "file" and optional "hotwords" (JSON list or comma-separated)
                  <- {"segments": [{"speaker", "start", "end", "text"}], "language", "duration",
                      "chunks", "warnings"}
POST /unload      -> frees the model immediately
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import shutil
import tempfile
import threading
import time
from contextlib import asynccontextmanager
from pathlib import Path
from typing import List, Optional

from fastapi import FastAPI, HTTPException, Request
from fastapi.concurrency import run_in_threadpool

from audio import AudioError, convert_to_wav, read_wav
from engine import Engine

__version__ = "0.1.0"

log = logging.getLogger("vibevoice.server")

ALLOWED_HOSTS = {"127.0.0.1", "localhost", "::1", "testserver"}
MAX_HOTWORDS = 200
MAX_HOTWORD_LEN = 100


def _env_float(name: str, default: float) -> float:
    try:
        return float(os.environ.get(name, default))
    except ValueError:
        return default


def build_engine() -> Engine:
    return Engine(
        model_id=os.environ.get("VIBEVOICE_MODEL", "mlx-community/VibeVoice-ASR-4bit"),
        idle_seconds=_env_float("VIBEVOICE_IDLE_MINUTES", 10) * 60,
        chunk_seconds=_env_float("VIBEVOICE_CHUNK_MINUTES", 25) * 60,
        overlap_seconds=_env_float("VIBEVOICE_OVERLAP_SECONDS", 30),
        max_tokens=int(_env_float("VIBEVOICE_MAX_TOKENS", 32768)),
    )


class Progress:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._state = {"active": False, "stage": "idle", "done": 0, "total": 0, "message": "", "started_at": None}

    def start(self) -> None:
        with self._lock:
            self._state = {"active": True, "stage": "converting", "done": 0, "total": 0,
                           "message": "Converting audio...", "started_at": time.time()}

    def update(self, stage: str, done: int, total: int, message: str) -> None:
        with self._lock:
            self._state.update(stage=stage, done=done, total=total, message=message)

    def finish(self) -> None:
        with self._lock:
            self._state.update(active=False, stage="idle")

    def snapshot(self) -> dict:
        with self._lock:
            return dict(self._state)


def normalize_hotwords(raw) -> List[str]:
    """Accept a list, a JSON list string or a comma/newline-separated string."""
    if raw is None:
        return []
    if isinstance(raw, str):
        raw = raw.strip()
        if not raw:
            return []
        try:
            parsed = json.loads(raw)
            raw = parsed if isinstance(parsed, list) else [raw]
        except json.JSONDecodeError:
            raw = [p for chunk in raw.splitlines() for p in chunk.split(",")]
    if not isinstance(raw, list):
        raise HTTPException(400, "hotwords must be a list of strings")
    out: List[str] = []
    seen = set()
    for item in raw:
        s = str(item).strip()[:MAX_HOTWORD_LEN]
        if s and s.lower() not in seen:
            seen.add(s.lower())
            out.append(s)
    return out[:MAX_HOTWORDS]


def create_app(engine: Optional[Engine] = None) -> FastAPI:
    state = {"engine": engine}
    progress = Progress()

    @asynccontextmanager
    async def lifespan(_app: FastAPI):
        if state["engine"] is None:
            state["engine"] = build_engine()
        yield
        state["engine"].shutdown()

    app = FastAPI(title="Meetily VibeVoice sidecar", version=__version__, lifespan=lifespan)

    @app.middleware("http")
    async def localhost_only(request: Request, call_next):
        # Defence against DNS rebinding: a web page cannot reach us under another hostname.
        host = (request.headers.get("host") or "").rsplit(":", 1)[0].strip("[]").lower()
        if host not in ALLOWED_HOSTS:
            from fastapi.responses import JSONResponse

            return JSONResponse({"detail": "Forbidden host"}, status_code=403)
        return await call_next(request)

    def get_engine() -> Engine:
        if state["engine"] is None:
            state["engine"] = build_engine()
        return state["engine"]

    @app.get("/health")
    def health() -> dict:
        eng = get_engine()
        try:
            from importlib.metadata import version

            mlx_audio_version = version("mlx-audio")
        except Exception:
            mlx_audio_version = None
        return {
            "status": "ok",
            "model": eng.model_id,
            "model_loaded": eng.loaded,
            "busy": eng.busy,
            "version": __version__,
            "mlx_audio_version": mlx_audio_version,
        }

    @app.get("/status")
    def status() -> dict:
        return progress.snapshot()

    @app.post("/unload")
    def unload() -> dict:
        eng = get_engine()
        if eng.busy:
            raise HTTPException(409, "A transcription is running")
        eng.unload()
        return {"model_loaded": False}

    @app.post("/transcribe")
    async def transcribe(request: Request) -> dict:
        eng = get_engine()
        if eng.busy:
            raise HTTPException(409, "Another transcription is already running")

        workdir = Path(tempfile.mkdtemp(prefix="vibevoice-"))
        try:
            ctype = request.headers.get("content-type", "")
            if ctype.startswith("multipart/form-data"):
                form = await request.form()
                upload = form.get("file")
                if upload is None or not hasattr(upload, "read"):
                    raise HTTPException(400, "multipart request needs a 'file' field")
                suffix = Path(getattr(upload, "filename", "") or "audio").suffix or ".bin"
                src = workdir / f"input{suffix}"
                with src.open("wb") as fh:
                    shutil.copyfileobj(upload.file, fh)
                hotwords = normalize_hotwords(form.get("hotwords"))
            else:
                try:
                    body = await request.json()
                except Exception:
                    raise HTTPException(400, "Body must be JSON or multipart/form-data")
                if not isinstance(body, dict) or not body.get("audio_path"):
                    raise HTTPException(400, "'audio_path' is required")
                src = Path(str(body["audio_path"])).expanduser()
                if not src.is_file():
                    raise HTTPException(404, "audio_path does not exist or is not a file")
                hotwords = normalize_hotwords(body.get("hotwords"))

            log.info("Transcription request received (%d hotwords)", len(hotwords))
            log.debug("Input file: %s", src)
            progress.start()
            try:
                def work() -> dict:
                    wav = convert_to_wav(src, workdir / "audio_24k.wav")
                    samples = read_wav(wav)
                    if samples.size == 0:
                        raise AudioError("audio file contains no samples")
                    return eng.transcribe(samples, hotwords, progress.update)

                t0 = time.monotonic()
                result = await run_in_threadpool(work)
                result["processing_seconds"] = round(time.monotonic() - t0, 2)
                log.info(
                    "Transcription finished: %.0fs audio, %d segments, %d chunk(s), %.1fs",
                    result["duration"], len(result["segments"]), result["chunks"],
                    result["processing_seconds"],
                )
                return result
            except AudioError as e:
                log.debug("Audio conversion error: %s", e)
                raise HTTPException(422, f"Could not read audio: {e}")
            except ImportError as e:
                raise HTTPException(
                    500, f"mlx-audio is not installed or failed to import ({e}). "
                         "Run ./start.sh to set up the environment."
                )
            except MemoryError:
                raise HTTPException(507, "Out of memory while running VibeVoice")
            finally:
                progress.finish()
        finally:
            shutil.rmtree(workdir, ignore_errors=True)

    return app


def main() -> None:
    parser = argparse.ArgumentParser(description="Meetily VibeVoice-ASR sidecar (localhost only)")
    parser.add_argument("--port", type=int, default=int(os.environ.get("VIBEVOICE_PORT", 8765)))
    parser.add_argument("--log-level", default=os.environ.get("VIBEVOICE_LOG_LEVEL", "info"))
    args = parser.parse_args()

    logging.basicConfig(level=args.log_level.upper(), format="%(asctime)s %(levelname)s %(name)s: %(message)s")
    import uvicorn

    # Host is intentionally not configurable: the API has no auth and reads local files.
    uvicorn.run(create_app(), host="127.0.0.1", port=args.port, log_level=args.log_level.lower())


if __name__ == "__main__":
    main()
