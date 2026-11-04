import math

import pytest

from parsing import (
    Segment,
    looks_truncated,
    merge_chunk_segments,
    normalize_speaker,
    parse_model_output,
    parse_time,
    plan_chunks,
    segments_from_mlx,
)

FULL = (
    '[{"Start time": 0.0, "End time": 3.5, "Speaker ID": 0, "Content": "Hello everyone."},'
    ' {"Start time": "3.6", "End time": "7.0", "Speaker ID": "1", "Content": "Hi, Acme update?"}]'
)


def test_parse_full_array():
    segs = parse_model_output(FULL)
    assert segs == [
        Segment("0", 0.0, 3.5, "Hello everyone."),
        Segment("1", 3.6, 7.0, "Hi, Acme update?"),
    ]


def test_parse_markdown_fenced():
    segs = parse_model_output("```json\n" + FULL + "\n```")
    assert len(segs) == 2


def test_parse_truncated_keeps_complete_objects():
    truncated = FULL[:-1] + ', {"Start time": 7.1, "End time": 9, "Speaker ID": 0, "Cont'
    segs = parse_model_output(truncated)
    assert [s.text for s in segs] == ["Hello everyone.", "Hi, Acme update?"]
    assert looks_truncated(truncated)
    assert not looks_truncated(FULL)


def test_parse_garbage_and_empty():
    assert parse_model_output("") == []
    assert parse_model_output("no json here") == []
    assert parse_model_output('[{"Start time": 1, "Content": ""}]') == []


def test_parse_single_object_and_alt_keys():
    segs = parse_model_output('{"Start": "00:01:02.5", "End": "1:03", "Speaker": "Speaker 2", "Content": "x"}')
    assert segs == [Segment("2", 62.5, 63.0, "x")]


def test_end_before_start_clamped():
    segs = parse_model_output('[{"Start time": 5, "End time": 2, "Speaker ID": 0, "Content": "x"}]')
    assert segs[0].end == 5


@pytest.mark.parametrize("raw,expected", [(0, "0"), (1.0, "1"), ("Speaker 3", "3"), ("speaker_A", "A"), (None, "unknown"), ("", "unknown")])
def test_normalize_speaker(raw, expected):
    assert normalize_speaker(raw) == expected


@pytest.mark.parametrize("raw,expected", [(1, 1.0), ("2.5", 2.5), ("2.5s", 2.5), ("01:30", 90.0), ("1:00:00", 3600.0), ("bad", None), (None, None), (True, None)])
def test_parse_time(raw, expected):
    assert parse_time(raw) == expected


def test_segments_from_mlx_shape():
    segs = segments_from_mlx([{"start": 0, "end": 1, "speaker_id": 0, "text": "a"}, "junk"])
    assert segs == [Segment("0", 0.0, 1.0, "a")]


def test_plan_single_chunk():
    chunks = plan_chunks(600, 1500, 30)
    assert len(chunks) == 1
    assert chunks[0].start == 0 and chunks[0].end == 600
    assert math.isinf(chunks[0].keep_until)


def test_plan_multiple_chunks_cover_and_overlap():
    duration = 3 * 3600
    chunks = plan_chunks(duration, 1500, 30)
    assert chunks[0].start == 0
    assert chunks[-1].end == duration
    for a, b in zip(chunks, chunks[1:]):
        assert b.start < a.end  # overlap
        assert a.keep_until == b.keep_from  # ownership boundary is shared
        assert b.start <= a.keep_until <= a.end
    assert all(c.end - c.start <= 1500 for c in chunks)


def test_plan_rejects_bad_args():
    assert plan_chunks(0, 100, 10) == []
    with pytest.raises(ValueError):
        plan_chunks(100, 0, 10)


def test_merge_dedupes_overlap_and_namespaces_speakers():
    chunks = plan_chunks(100, 60, 20)  # [0,60] and [40,100], boundary at 50
    assert len(chunks) == 2
    first = [Segment("0", 45, 48, "kept in chunk 1"), Segment("0", 52, 55, "dup, owned by chunk 2")]
    second = [Segment("0", 5, 8, "dup, owned by chunk 1"), Segment("1", 12, 15, "kept in chunk 2")]
    merged = merge_chunk_segments(chunks[0], first, 2) + merge_chunk_segments(chunks[1], second, 2)
    assert [(s.speaker, s.start, s.text) for s in merged] == [
        ("1-0", 45, "kept in chunk 1"),
        ("2-1", 52, "kept in chunk 2"),
    ]


def test_merge_single_chunk_keeps_plain_ids():
    chunks = plan_chunks(30, 60, 10)
    merged = merge_chunk_segments(chunks[0], [Segment("0", 1, 2, "a")], 1)
    assert merged[0].speaker == "0"
