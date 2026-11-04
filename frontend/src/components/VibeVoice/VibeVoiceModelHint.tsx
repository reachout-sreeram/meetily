"use client";

import { useEffect, useState } from 'react';
import { checkVibeVoiceHealth, VIBEVOICE_START_COMMAND, type VibeVoiceHealthStatus } from './api';

/** Status line shown under the model selector when VibeVoice is selected. */
export function VibeVoiceModelHint() {
  const [health, setHealth] = useState<VibeVoiceHealthStatus | null>(null);
  const [checking, setChecking] = useState(true);

  const check = async () => {
    setChecking(true);
    try {
      setHealth(await checkVibeVoiceHealth());
    } catch (err) {
      setHealth({
        reachable: false,
        serverUrl: '',
        modelLoaded: false,
        busy: false,
        message: err instanceof Error ? err.message : String(err),
      });
    } finally {
      setChecking(false);
    }
  };

  useEffect(() => {
    check();
  }, []);

  return (
    <div className="rounded-md border border-gray-200 bg-gray-50 p-2 text-xs space-y-1">
      <p className="text-muted-foreground">
        Post-meeting only. Produces speaker labels and timestamps using the local VibeVoice server
        (Apple Silicon). Hotwords and the server URL are in Settings → Transcription.
      </p>
      {checking ? (
        <p className="text-muted-foreground">Checking VibeVoice server…</p>
      ) : health?.reachable ? (
        <p className="text-green-700">● {health.message}</p>
      ) : (
        <div className="text-red-700 space-y-1">
          <p>● {health?.message || 'VibeVoice server is not running.'}</p>
          <p className="text-gray-700">
            Start it in a terminal: <code className="rounded bg-white px-1">{VIBEVOICE_START_COMMAND}</code>{' '}
            <button type="button" className="underline" onClick={check}>
              Check again
            </button>
          </p>
        </div>
      )}
    </div>
  );
}
