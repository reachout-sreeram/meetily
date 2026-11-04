"""Lazy-loading wrapper around the mlx-audio VibeVoice-ASR model.

The model (~5.7 GB for the 4-bit build) is loaded on first use and unloaded
after ``idle_seconds`` without a request, so a 16 GB Mac gets its RAM back.
"""

from __future__ import annotations

import gc
import logging
import threading
import time
from typing import Any, Callable, List, Optional

import numpy as np

from audio import SAMPLE_RATE
from parsing import (
    Segment,
    looks_truncated,
    merge_chunk_segments,
    parse_model_output,
    plan_chunks,
    segments_from_mlx,
)

log = logging.getLogger("vibevoice.engine")

ProgressFn = Callable[[str, int, int, str], None]  # stage, done, total, message


class Engine:
    def __init__(
        self,
        model_id: str,
        idle_seconds: float,
        chunk_seconds: float,
        overlap_seconds: float,
        max_tokens: int,
    ) -> None:
        self.model_id = model_id
        self.idle_seconds = idle_seconds
        self.chunk_seconds = chunk_seconds
        self.overlap_seconds = overlap_seconds
        self.max_tokens = max_tokens
        self._model: Any = None
        self._lock = threading.Lock()  # one inference at a time; guards _model
        self._last_used = time.monotonic()
        self._stop = threading.Event()
        self._reaper = threading.Thread(target=self._reap_loop, name="vibevoice-idle", daemon=True)
        self._reaper.start()

    # ---- lifecycle -------------------------------------------------------
    @property
    def loaded(self) -> bool:
        return self._model is not None

    @property
    def busy(self) -> bool:
        return self._lock.locked()

    def _load_locked(self) -> Any:
        if self._model is None:
            from mlx_audio.stt import load  # imported lazily: heavy + macOS only

            log.info("Loading model %s", self.model_id)
            t0 = time.monotonic()
            self._model = load(self.model_id)
            log.info("Model loaded in %.1fs", time.monotonic() - t0)
        return self._model

    def _unload_locked(self) -> None:
        if self._model is None:
            return
        self._model = None
        gc.collect()
        try:
            import mlx.core as mx

            mx.clear_cache()
        except Exception:  # pragma: no cover - mlx missing or older API
            pass
        log.info("Model unloaded")

    def unload(self) -> None:
        with self._lock:
            self._unload_locked()

    def _reap_loop(self) -> None:
        while not self._stop.wait(15):
            if self.idle_seconds <= 0 or self._model is None:
                continue
            if time.monotonic() - self._last_used < self.idle_seconds:
                continue
            if self._lock.acquire(blocking=False):
                try:
                    if self._model is not None and time.monotonic() - self._last_used >= self.idle_seconds:
                        log.info("Idle for %.0fs, unloading", self.idle_seconds)
                        self._unload_locked()
                finally:
                    self._lock.release()

    def shutdown(self) -> None:
        self._stop.set()
        self.unload()

    # ---- inference -------------------------------------------------------
    def transcribe(
        self,
        samples: np.ndarray,
        hotwords: Optional[List[str]],
        progress: ProgressFn,
    ) -> dict:
        duration = len(samples) / SAMPLE_RATE
        chunks = plan_chunks(duration, self.chunk_seconds, self.overlap_seconds)
        warnings: List[str] = []
        if len(chunks) > 1:
            warnings.append(
                f"Recording is {duration / 60:.1f} min and was split into {len(chunks)} chunks of "
                f"~{self.chunk_seconds / 60:.0f} min. Speaker IDs are assigned per chunk, so they are "
                "prefixed with the chunk number (e.g. '2-0') and the same person may appear under "
                "several IDs. Rename them to the same name to merge them."
            )

        with self._lock:
            progress("loading", 0, len(chunks), "Loading VibeVoice model...")
            model = self._load_locked()
            all_segments: List[Segment] = []
            try:
                for chunk in chunks:
                    progress(
                        "transcribing", chunk.index, len(chunks),
                        f"Transcribing chunk {chunk.index + 1} of {len(chunks)}...",
                    )
                    a = int(chunk.start * SAMPLE_RATE)
                    b = int(chunk.end * SAMPLE_RATE)
                    t0 = time.monotonic()
                    result = model.generate(
                        samples[a:b],
                        sampling_rate=SAMPLE_RATE,
                        hotwords=hotwords or None,
                        max_tokens=self.max_tokens,
                    )
                    text = getattr(result, "text", "") or ""
                    segs = parse_model_output(text) or segments_from_mlx(getattr(result, "segments", None))
                    if looks_truncated(text):
                        warnings.append(
                            f"Chunk {chunk.index + 1}: model output was cut off at the token limit "
                            f"({self.max_tokens}); the end of this chunk may be missing. "
                            "Lower VIBEVOICE_CHUNK_MINUTES or raise VIBEVOICE_MAX_TOKENS."
                        )
                    log.info(
                        "Chunk %d/%d: %.0fs audio -> %d segments in %.1fs (%s tokens)",
                        chunk.index + 1, len(chunks), chunk.end - chunk.start, len(segs),
                        time.monotonic() - t0, getattr(result, "generation_tokens", "?"),
                    )
                    all_segments.extend(merge_chunk_segments(chunk, segs, len(chunks)))
            finally:
                self._last_used = time.monotonic()

        all_segments.sort(key=lambda s: (s.start, s.end))
        progress("done", len(chunks), len(chunks), "Done")
        return {
            "segments": [s.to_dict() for s in all_segments],
            "language": None,  # VibeVoice does not report the detected language
            "duration": round(duration, 3),
            "chunks": len(chunks),
            "warnings": warnings,
        }
