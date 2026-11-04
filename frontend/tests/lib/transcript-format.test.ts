import { describe, expect, test } from 'bun:test';
import { formatRecordingTimestamp, formatTranscriptLine } from '../../src/lib/transcript-format';

describe('transcript line formatting', () => {
  test('formats recording-relative timestamps', () => {
    expect(formatRecordingTimestamp(0, 'x')).toBe('[00:00]');
    expect(formatRecordingTimestamp(125.9, 'x')).toBe('[02:05]');
    expect(formatRecordingTimestamp(undefined, '14:30:05')).toBe('14:30:05');
  });

  test('prefixes renamed speaker names for summaries', () => {
    expect(
      formatTranscriptLine({ audio_start_time: 61, timestamp: 't', text: 'I will send the deck.', speaker_name: 'Priya' }),
    ).toBe('[01:01] Priya: I will send the deck.');
    expect(
      formatTranscriptLine({ audio_start_time: 3, timestamp: 't', text: 'Hi', speaker_name: 'Speaker 1' }),
    ).toBe('[00:03] Speaker 1: Hi');
  });

  test('legacy transcripts without speakers are unchanged', () => {
    expect(formatTranscriptLine({ audio_start_time: 5, timestamp: 't', text: 'hello' })).toBe('[00:05] hello');
    expect(formatTranscriptLine({ audio_start_time: undefined, timestamp: '10:00:00', text: 'old', speaker_name: '  ' })).toBe(
      '10:00:00 old',
    );
  });
});
