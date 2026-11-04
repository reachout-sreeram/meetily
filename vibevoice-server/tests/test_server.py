import shutil
import subprocess

import numpy as np
import pytest
from fastapi.testclient import TestClient

import server


class FakeEngine:
    model_id = "fake/model"
    loaded = False
    busy = False

    def __init__(self):
        self.calls = []

    def transcribe(self, samples, hotwords, progress):
        self.calls.append((len(samples), hotwords))
        progress("transcribing", 0, 1, "x")
        return {"segments": [{"speaker": "0", "start": 0.0, "end": 1.0, "text": "hi"}],
                "language": None, "duration": len(samples) / 24000, "chunks": 1, "warnings": []}

    def unload(self):
        pass

    def shutdown(self):
        pass


@pytest.fixture
def client():
    eng = FakeEngine()
    with TestClient(server.create_app(eng)) as c:
        c.engine = eng
        yield c


def test_health(client):
    r = client.get("/health")
    assert r.status_code == 200
    body = r.json()
    assert body["status"] == "ok" and body["model_loaded"] is False and body["version"]


def test_rejects_foreign_host(client):
    r = client.get("/health", headers={"host": "evil.example:8765"})
    assert r.status_code == 403


def test_transcribe_requires_path(client):
    assert client.post("/transcribe", json={}).status_code == 400
    assert client.post("/transcribe", json={"audio_path": "/nope/x.wav"}).status_code == 404


@pytest.mark.skipif(shutil.which("ffmpeg") is None, reason="ffmpeg not installed")
def test_transcribe_path_and_upload(client, tmp_path):
    src = tmp_path / "tone.mp3"
    subprocess.run(["ffmpeg", "-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=2",
                    "-ac", "2", "-ar", "44100", str(src)], check=True)
    r = client.post("/transcribe", json={"audio_path": str(src), "hotwords": ["Acme", " acme ", "", "Jira"]})
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["segments"][0] == {"speaker": "0", "start": 0.0, "end": 1.0, "text": "hi"}
    n, hot = client.engine.calls[-1]
    assert hot == ["Acme", "Jira"]
    assert abs(n / 24000 - 2.0) < 0.1  # converted to 24 kHz mono

    with src.open("rb") as fh:
        r = client.post("/transcribe", files={"file": ("tone.mp3", fh, "audio/mpeg")}, data={"hotwords": "A, B"})
    assert r.status_code == 200, r.text
    assert client.engine.calls[-1][1] == ["A", "B"]


def test_bad_audio_returns_422(client, tmp_path):
    if shutil.which("ffmpeg") is None:
        pytest.skip("ffmpeg not installed")
    bad = tmp_path / "bad.wav"
    bad.write_bytes(b"not audio")
    r = client.post("/transcribe", json={"audio_path": str(bad)})
    assert r.status_code == 422


def test_normalize_hotwords_variants():
    assert server.normalize_hotwords('["a","b"]') == ["a", "b"]
    assert server.normalize_hotwords("a\nb, c") == ["a", "b", "c"]
    assert server.normalize_hotwords(None) == []
    assert len(server.normalize_hotwords([str(i) for i in range(500)])) == server.MAX_HOTWORDS
