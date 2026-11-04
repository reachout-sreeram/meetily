"""Pure helpers for turning VibeVoice-ASR output into transcript segments.

No MLX imports here, so this module is unit-testable on any platform.

VibeVoice-ASR emits a JSON array such as::

    [{"Start time": 0.0, "End time": 3.2, "Speaker ID": 0, "Content": "Hi all"}, ...]

mlx-audio's own ``parse_transcription`` returns ``[]`` when the JSON is
incomplete (for example when generation stops at ``max_tokens``), which would
silently drop the whole transcript. ``parse_model_output`` instead recovers
every complete object it can find.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, asdict
from typing import Any, Iterable, List, Optional

_START_KEYS = ("Start time", "Start", "start", "start_time")
_END_KEYS = ("End time", "End", "end", "end_time")
_SPEAKER_KEYS = ("Speaker ID", "Speaker", "speaker_id", "speaker")
_TEXT_KEYS = ("Content", "content", "text", "Text")


@dataclass
class Segment:
    speaker: str
    start: float
    end: float
    text: str

    def to_dict(self) -> dict:
        return asdict(self)


def parse_time(value: Any) -> Optional[float]:
    """Parse seconds given as a number, "12.5", "01:02.5" or "1:02:03.5"."""
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return float(value)
    s = str(value).strip().rstrip("s").strip()
    if not s:
        return None
    try:
        if ":" in s:
            total = 0.0
            for part in s.split(":"):
                total = total * 60 + float(part)
            return total
        return float(s)
    except ValueError:
        return None


def _first(item: dict, keys: Iterable[str]) -> Any:
    for k in keys:
        if k in item:
            return item[k]
    return None


def normalize_speaker(value: Any) -> str:
    """Return a stable string speaker id: 0 -> "0", "Speaker 1" -> "1"."""
    if value is None:
        return "unknown"
    if isinstance(value, float) and value.is_integer():
        value = int(value)
    s = str(value).strip()
    m = re.fullmatch(r"(?i)speaker[\s_-]*(\w+)", s)
    if m:
        s = m.group(1)
    return s or "unknown"


def _iter_json_objects(text: str) -> Iterable[dict]:
    """Yield every complete top-level JSON object found in ``text``.

    Works on truncated arrays: ``[{...}, {...}, {"Start ti`` yields the two
    complete objects.
    """
    decoder = json.JSONDecoder()
    i = 0
    n = len(text)
    while i < n:
        j = text.find("{", i)
        if j == -1:
            return
        try:
            obj, end = decoder.raw_decode(text, j)
        except json.JSONDecodeError:
            i = j + 1
            continue
        if isinstance(obj, dict):
            yield obj
        i = end


def parse_model_output(text: str) -> List[Segment]:
    """Parse raw VibeVoice text into segments, tolerating truncation."""
    if not text:
        return []
    # Strip markdown fences if present.
    cleaned = re.sub(r"```(?:json)?", "", text)
    segments: List[Segment] = []
    for obj in _iter_json_objects(cleaned):
        seg = segment_from_dict(obj)
        if seg is not None:
            segments.append(seg)
    return segments


def segment_from_dict(obj: dict) -> Optional[Segment]:
    content = _first(obj, _TEXT_KEYS)
    if content is None:
        return None
    content = str(content).strip()
    if not content:
        return None
    start = parse_time(_first(obj, _START_KEYS))
    end = parse_time(_first(obj, _END_KEYS))
    if start is None:
        start = 0.0
    if end is None or end < start:
        end = start
    return Segment(
        speaker=normalize_speaker(_first(obj, _SPEAKER_KEYS)),
        start=start,
        end=end,
        text=content,
    )


def segments_from_mlx(segments: Optional[list]) -> List[Segment]:
    """Convert mlx-audio's ``STTOutput.segments`` (already key-mapped)."""
    out: List[Segment] = []
    for item in segments or []:
        if isinstance(item, dict):
            seg = segment_from_dict(item)
            if seg is not None:
                out.append(seg)
    return out


def looks_truncated(text: str) -> bool:
    """True if the output does not end with a closed JSON array/object."""
    t = re.sub(r"```(?:json)?", "", text or "").strip()
    return bool(t) and not (t.endswith("]") or t.endswith("}"))


@dataclass
class ChunkSpec:
    index: int
    start: float  # seconds into the full recording
    end: float
    keep_from: float  # absolute seconds: keep segments starting at/after this
    keep_until: float  # absolute seconds: keep segments starting before this


def plan_chunks(duration: float, chunk_seconds: float, overlap_seconds: float) -> List[ChunkSpec]:
    """Split ``duration`` into overlapping windows.

    Each segment is owned by exactly one chunk: the ownership boundary between
    neighbouring chunks is the middle of their overlap.
    """
    if duration <= 0:
        return []
    if chunk_seconds <= 0:
        raise ValueError("chunk_seconds must be > 0")
    overlap_seconds = max(0.0, min(overlap_seconds, chunk_seconds / 2))
    if duration <= chunk_seconds:
        return [ChunkSpec(0, 0.0, duration, 0.0, float("inf"))]

    step = chunk_seconds - overlap_seconds
    starts: List[float] = []
    s = 0.0
    while True:
        starts.append(s)
        if s + chunk_seconds >= duration:
            break
        s += step

    chunks: List[ChunkSpec] = []
    for i, start in enumerate(starts):
        end = min(start + chunk_seconds, duration)
        keep_from = 0.0 if i == 0 else (start + (starts[i - 1] + chunk_seconds)) / 2
        if i == len(starts) - 1:
            keep_until = float("inf")
        else:
            nxt = starts[i + 1]
            keep_until = (nxt + end) / 2
        chunks.append(ChunkSpec(i, start, end, keep_from, keep_until))
    return chunks


def merge_chunk_segments(chunk: ChunkSpec, segments: List[Segment], total_chunks: int) -> List[Segment]:
    """Shift chunk-relative times to absolute, drop overlap duplicates and,
    when there is more than one chunk, namespace speaker ids per chunk.

    VibeVoice assigns speaker ids per pass, so "0" in chunk 1 is not
    necessarily "0" in chunk 2. Namespacing ("1-0", "2-0") makes this visible
    instead of silently attributing speech to the wrong person.
    """
    out: List[Segment] = []
    for seg in segments:
        start = seg.start + chunk.start
        end = seg.end + chunk.start
        if start < chunk.keep_from or start >= chunk.keep_until:
            continue
        speaker = seg.speaker if total_chunks == 1 else f"{chunk.index + 1}-{seg.speaker}"
        out.append(Segment(speaker=speaker, start=round(start, 3), end=round(end, 3), text=seg.text))
    return out
