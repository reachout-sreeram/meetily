import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import type { VibeVoiceWarningEvent } from './api';

/** Show VibeVoice sidecar warnings (long-recording chunking, token-limit truncation) as toasts. */
export function useVibeVoiceWarnings(active: boolean) {
  useEffect(() => {
    if (!active) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<VibeVoiceWarningEvent>('vibevoice-warning', (event) => {
      for (const warning of event.payload.warnings) {
        toast.warning('VibeVoice', { description: warning, duration: 15000 });
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [active]);
}
