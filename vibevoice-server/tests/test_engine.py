import json
from types import SimpleNamespace

import numpy as np

from engine import Engine


class FakeModel:
    """Returns one segment at chunk-relative 5s; truncates output on request."""

    def __init__(self, truncate=False):
        self.truncate = truncate
        self.calls = []

    def generate(self, audio, sampling_rate, hotwords, max_tokens):
        self.calls.append((len(audio) / sampling_rate, hotwords, max_tokens))
        text = json.dumps([{"Start time": 5.0, "End time": 6.0, "Speaker ID": 0, "Content": f"c{len(self.calls)}"}])
        if self.truncate:
            text = text[:-1] + ', {"Start time": 7'
        return SimpleNamespace(text=text, segments=[], generation_tokens=10)


def make_engine(model, chunk_s=60, overlap_s=10):
    eng = Engine("fake", idle_seconds=0, chunk_seconds=chunk_s, overlap_seconds=overlap_s, max_tokens=1234)
    eng._model = model
    return eng


def test_single_chunk_plain_speakers():
    model = FakeModel()
    eng = make_engine(model)
    events = []
    out = eng.transcribe(np.zeros(24000 * 30, dtype=np.float32), ["Acme"], lambda *a: events.append(a))
    assert out["chunks"] == 1 and out["warnings"] == []
    assert out["segments"] == [{"speaker": "0", "start": 5.0, "end": 6.0, "text": "c1"}]
    assert model.calls == [(30.0, ["Acme"], 1234)]
    assert events[-1][0] == "done"
    eng.shutdown()


def test_multi_chunk_offsets_and_warning():
    model = FakeModel()
    eng = make_engine(model, chunk_s=60, overlap_s=10)
    out = eng.transcribe(np.zeros(24000 * 150, dtype=np.float32), None, lambda *a: None)
    assert out["chunks"] == 3
    assert [s["start"] for s in out["segments"]] == [5.0, 55.0, 105.0]
    assert [s["speaker"] for s in out["segments"]] == ["1-0", "2-0", "3-0"]
    assert any("split into 3 chunks" in w for w in out["warnings"])
    eng.shutdown()


def test_truncated_output_is_reported_not_dropped():
    eng = make_engine(FakeModel(truncate=True))
    out = eng.transcribe(np.zeros(24000 * 10, dtype=np.float32), None, lambda *a: None)
    assert len(out["segments"]) == 1
    assert any("cut off" in w for w in out["warnings"])
    eng.shutdown()


def test_unload_clears_model():
    eng = make_engine(FakeModel())
    assert eng.loaded
    eng.unload()
    assert not eng.loaded
    eng.shutdown()
