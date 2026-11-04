import { invoke } from '@tauri-apps/api/core';

export const VIBEVOICE_START_COMMAND = 'cd vibevoice-server && ./start.sh';

export interface VibeVoiceSettings {
  serverUrl: string;
  hotwords: string[];
}

export interface VibeVoiceHealthStatus {
  reachable: boolean;
  serverUrl: string;
  model?: string | null;
  modelLoaded: boolean;
  busy: boolean;
  version?: string | null;
  message: string;
}

export interface MeetingSpeaker {
  speaker_id: string;
  display_name: string;
  is_custom: boolean;
  segment_count: number;
  total_seconds: number;
}

export interface VibeVoiceWarningEvent {
  meeting_id: string | null;
  warnings: string[];
}

export const getVibeVoiceSettings = () => invoke<VibeVoiceSettings>('vibevoice_get_settings');

export const saveVibeVoiceSettings = (settings: VibeVoiceSettings) =>
  invoke<VibeVoiceSettings>('vibevoice_save_settings', { settings });

export const checkVibeVoiceHealth = (serverUrl?: string) =>
  invoke<VibeVoiceHealthStatus>('vibevoice_health_check', { serverUrl: serverUrl ?? null });

export const getMeetingSpeakers = (meetingId: string) =>
  invoke<MeetingSpeaker[]>('vibevoice_get_meeting_speakers', { meetingId });

export const renameMeetingSpeaker = (meetingId: string, speakerId: string, displayName: string | null) =>
  invoke<MeetingSpeaker[]>('vibevoice_rename_speaker', { meetingId, speakerId, displayName });

/** Split a textarea value (one per line or comma-separated) into hotwords. */
export function parseHotwords(raw: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const part of raw.split(/[\n,]/)) {
    const word = part.trim();
    if (word && !seen.has(word.toLowerCase())) {
      seen.add(word.toLowerCase());
      out.push(word);
    }
  }
  return out;
}
