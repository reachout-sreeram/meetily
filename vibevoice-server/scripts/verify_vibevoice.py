#!/usr/bin/env python3
"""Step 0: check VibeVoice-ASR via mlx-audio on this Mac before trusting the app.

Usage (from vibevoice-server/, after ./start.sh has created .venv):

    .venv/bin/python scripts/verify_vibevoice.py path/to/10min_multi_speaker.wav \
        --hotwords "Acme,Jira" [--skip-cli]

Reports:
  * raw output text (first 1500 chars) and the parsed segment structure
  * whether STTOutput.segments is populated or we must parse text
  * whether hotwords are accepted
  * wall time, real-time factor and peak memory (MLX peak + process max RSS)
"""

import argparse
import json
import resource
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from audio import SAMPLE_RATE, convert_to_wav, read_wav  # noqa: E402
from parsing import looks_truncated, parse_model_output  # noqa: E402

MODEL = "mlx-community/VibeVoice-ASR-4bit"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("audio")
    ap.add_argument("--model", default=MODEL)
    ap.add_argument("--hotwords", default="")
    ap.add_argument("--max-tokens", type=int, default=32768)
    ap.add_argument("--skip-cli", action="store_true")
    args = ap.parse_args()
    hotwords = [h.strip() for h in args.hotwords.split(",") if h.strip()]
    out_dir = Path("verify_out")
    out_dir.mkdir(exist_ok=True)

    if not args.skip_cli:
        print("== 1. CLI ==")
        cmd = [sys.executable, "-m", "mlx_audio.stt.generate", "--model", args.model,
               "--audio", args.audio, "--output-path", str(out_dir / "cli"), "--format", "json",
               "--max-tokens", str(args.max_tokens)]
        if hotwords:
            cmd += ["--context", ", ".join(hotwords)]
        t0 = time.time()
        rc = subprocess.run(cmd).returncode
        print(f"CLI exit={rc} in {time.time() - t0:.1f}s; outputs in {out_dir}/")

    print("\n== 2. Python API ==")
    import mlx.core as mx
    from mlx_audio.stt.utils import load_model

    wav = convert_to_wav(Path(args.audio), out_dir / "audio_24k.wav")
    samples = read_wav(wav)
    duration = len(samples) / SAMPLE_RATE
    print(f"audio duration: {duration / 60:.1f} min")

    t0 = time.time()
    model = load_model(args.model)
    load_s = time.time() - t0
    print(f"model load: {load_s:.1f}s")

    try:
        mx.reset_peak_memory()
    except Exception:
        pass
    t0 = time.time()
    result = model.generate(samples, sampling_rate=SAMPLE_RATE, hotwords=hotwords or None, max_tokens=args.max_tokens)
    gen_s = time.time() - t0

    text = result.text or ""
    print("\n-- raw text (first 1500 chars) --")
    print(text[:1500])
    print("\n-- STTOutput.segments (first 3) --")
    print(json.dumps((result.segments or [])[:3], indent=2, ensure_ascii=False))
    parsed = parse_model_output(text)
    print(f"\nsegments: mlx={len(result.segments or [])} tolerant-parser={len(parsed)}")
    print(f"speakers: {sorted({s.speaker for s in parsed})}")
    print(f"output truncated at token limit: {looks_truncated(text)}")
    print(f"generation tokens: {getattr(result, 'generation_tokens', '?')} (limit {args.max_tokens})")

    try:
        peak_gb = mx.get_peak_memory() / 1e9
    except Exception:
        peak_gb = float("nan")
    rss_gb = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1e9  # bytes on macOS
    print(f"\n== RESULT ==\ninference: {gen_s:.1f}s for {duration:.0f}s audio (RTF {gen_s / max(duration, 1e-9):.3f})")
    print(f"MLX peak memory: {peak_gb:.2f} GB   process max RSS: {rss_gb:.2f} GB")
    (out_dir / "api_result.json").write_text(json.dumps(
        {"text": text, "segments": [s.to_dict() for s in parsed], "inference_s": gen_s,
         "load_s": load_s, "duration_s": duration, "mlx_peak_gb": peak_gb, "max_rss_gb": rss_gb},
        indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
