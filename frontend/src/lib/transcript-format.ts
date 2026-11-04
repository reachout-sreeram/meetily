import type { Transcript } from '@/types';

/** Recording-relative "[MM:SS]", or the wall-clock fallback for old transcripts. */
export function formatRecordingTimestamp(seconds: number | undefined, fallbackTimestamp: string): string {
  if (seconds === undefined || seconds === null) {
    return fallbackTimestamp;
  }
  const totalSecs = Math.floor(seconds);
  const mins = Math.floor(totalSecs / 60);
  const secs = totalSecs % 60;
  return `[${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
}

/**
 * One transcript line for summaries and copies: "[MM:SS] Name: text".
 * The speaker prefix appears only for speaker-labelled (VibeVoice) transcripts and uses the
 * per-meeting renamed name, so the LLM can attribute decisions and action items to people.
 * Lines without speakers are unchanged from before.
 */
export function formatTranscriptLine(t: Pick<Transcript, 'audio_start_time' | 'timestamp' | 'text' | 'speaker_name'>): string {
  const time = formatRecordingTimestamp(t.audio_start_time, t.timestamp);
  const speaker = t.speaker_name?.trim();
  return speaker ? `${time} ${speaker}: ${t.text}` : `${time} ${t.text}`;
}
