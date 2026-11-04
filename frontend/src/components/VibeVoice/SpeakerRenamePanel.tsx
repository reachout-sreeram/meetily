"use client";

import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Users, Check, RotateCcw } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { getMeetingSpeakers, renameMeetingSpeaker, type MeetingSpeaker } from './api';

interface SpeakerRenamePanelProps {
  meetingId: string;
  /** Changes whenever transcripts are reloaded, so the speaker list refreshes too. */
  refreshKey?: unknown;
  /** Called after a rename so the transcript view picks up the new names. */
  onRenamed?: () => void | Promise<void>;
}

function formatDuration(seconds: number): string {
  const s = Math.round(seconds);
  return s >= 60 ? `${Math.floor(s / 60)}m ${s % 60}s` : `${s}s`;
}

/**
 * Rename speakers once per meeting. Names are applied to every segment of that speaker,
 * in the transcript view, copies and summaries. Hidden when the meeting has no speaker labels.
 */
export function SpeakerRenamePanel({ meetingId, refreshKey, onRenamed }: SpeakerRenamePanelProps) {
  const [speakers, setSpeakers] = useState<MeetingSpeaker[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [expanded, setExpanded] = useState(false);
  const [savingId, setSavingId] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await getMeetingSpeakers(meetingId);
      setSpeakers(list);
      setDrafts(Object.fromEntries(list.map((s) => [s.speaker_id, s.is_custom ? s.display_name : ''])));
    } catch (err) {
      console.error('Failed to load speakers:', err);
      setSpeakers([]);
    }
  }, [meetingId]);

  useEffect(() => {
    load();
  }, [load, refreshKey]);

  if (speakers.length === 0) return null;

  const save = async (speakerId: string, name: string | null) => {
    setSavingId(speakerId);
    try {
      const list = await renameMeetingSpeaker(meetingId, speakerId, name);
      setSpeakers(list);
      setDrafts((d) => ({ ...d, [speakerId]: name ?? '' }));
      await onRenamed?.();
    } catch (err) {
      toast.error('Could not rename speaker', { description: err instanceof Error ? err.message : String(err) });
    } finally {
      setSavingId(null);
    }
  };

  const hasChunkedIds = speakers.some((s) => s.speaker_id.includes('-'));

  return (
    <div className="px-4 py-2 border-b border-gray-200 bg-gray-50">
      <button
        type="button"
        className="flex w-full items-center gap-2 text-sm font-medium text-gray-700"
        onClick={() => setExpanded((v) => !v)}
        aria-expanded={expanded}
      >
        <Users className="h-4 w-4" />
        Speakers ({speakers.length})
        <span className="ml-auto text-xs font-normal text-blue-600">{expanded ? 'Hide' : 'Rename'}</span>
      </button>
      {expanded && (
        <div className="mt-2 space-y-2">
          {hasChunkedIds && (
            <p className="text-xs text-amber-700">
              This long recording was transcribed in chunks, so one person may appear under several IDs
              (e.g. 1-0 and 2-0). Give them the same name to merge them.
            </p>
          )}
          {speakers.map((s) => {
            const draft = drafts[s.speaker_id] ?? '';
            const dirty = draft.trim() !== (s.is_custom ? s.display_name : '');
            return (
              <form
                key={s.speaker_id}
                className="flex items-center gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  save(s.speaker_id, draft.trim() || null);
                }}
              >
                <span className="w-24 shrink-0 text-xs text-gray-500" title={`${s.segment_count} segments`}>
                  Speaker {s.speaker_id} · {formatDuration(s.total_seconds)}
                </span>
                <Input
                  className="h-8 text-sm"
                  value={draft}
                  placeholder={`Speaker ${s.speaker_id}`}
                  maxLength={80}
                  onChange={(e) => setDrafts((d) => ({ ...d, [s.speaker_id]: e.target.value }))}
                />
                <Button type="submit" size="sm" variant="outline" disabled={!dirty || savingId === s.speaker_id} title="Save name">
                  <Check className="h-4 w-4" />
                </Button>
                {s.is_custom && (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    disabled={savingId === s.speaker_id}
                    title="Reset to default"
                    onClick={() => save(s.speaker_id, null)}
                  >
                    <RotateCcw className="h-4 w-4" />
                  </Button>
                )}
              </form>
            );
          })}
        </div>
      )}
    </div>
  );
}
