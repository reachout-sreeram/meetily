"use client";

import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import {
  checkVibeVoiceHealth,
  getVibeVoiceSettings,
  parseHotwords,
  saveVibeVoiceSettings,
  VIBEVOICE_START_COMMAND,
  type VibeVoiceHealthStatus,
} from './api';

/** Settings card for the local VibeVoice-ASR sidecar (server URL + hotwords). */
export function VibeVoiceSettings() {
  const [serverUrl, setServerUrl] = useState('');
  const [hotwordsText, setHotwordsText] = useState('');
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [health, setHealth] = useState<VibeVoiceHealthStatus | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    getVibeVoiceSettings()
      .then((s) => {
        setServerUrl(s.serverUrl);
        setHotwordsText(s.hotwords.join('\n'));
      })
      .catch((err) => console.error('Failed to load VibeVoice settings:', err))
      .finally(() => setLoaded(true));
  }, []);

  const handleTest = async () => {
    setTesting(true);
    try {
      setHealth(await checkVibeVoiceHealth(serverUrl));
    } catch (err) {
      setHealth(null);
      toast.error(err instanceof Error ? err.message : String(err));
    } finally {
      setTesting(false);
    }
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      const saved = await saveVibeVoiceSettings({ serverUrl, hotwords: parseHotwords(hotwordsText) });
      setServerUrl(saved.serverUrl);
      setHotwordsText(saved.hotwords.join('\n'));
      toast.success('VibeVoice settings saved');
    } catch (err) {
      toast.error('Could not save VibeVoice settings', {
        description: err instanceof Error ? err.message : String(err),
      });
    } finally {
      setSaving(false);
    }
  };

  const hotwordCount = parseHotwords(hotwordsText).length;

  return (
    <div className="mt-6 max-w-3xl rounded-lg border border-gray-200 bg-white p-4 space-y-4">
      <div>
        <h3 className="text-base font-semibold">VibeVoice (local server) — post-meeting speaker labels</h3>
        <p className="text-sm text-muted-foreground">
          Gives each line of a finished meeting a speaker label (Speaker 0, Speaker 1, …) that you can rename.
          Use it from a meeting&apos;s <strong>Enhance</strong> button or when importing audio; it is never used for live
          transcription. Requires the local sidecar on an Apple
          Silicon Mac: <code className="rounded bg-gray-100 px-1">{VIBEVOICE_START_COMMAND}</code>
        </p>
      </div>

      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor="vibevoice-url">Server URL</label>
        <div className="flex gap-2">
          <Input
            id="vibevoice-url"
            value={serverUrl}
            disabled={!loaded}
            placeholder="http://127.0.0.1:8765"
            onChange={(e) => setServerUrl(e.target.value)}
          />
          <Button variant="outline" onClick={handleTest} disabled={!loaded || testing}>
            {testing ? 'Testing…' : 'Test connection'}
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">Only localhost addresses are accepted; audio never leaves this computer.</p>
        {health && (
          <p className={`text-xs ${health.reachable ? 'text-green-700' : 'text-red-700'}`}>
            ● {health.message}
            {health.reachable && health.version ? ` (server v${health.version})` : ''}
          </p>
        )}
      </div>

      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor="vibevoice-hotwords">
          Hotwords <span className="font-normal text-muted-foreground">({hotwordCount})</span>
        </label>
        <Textarea
          id="vibevoice-hotwords"
          value={hotwordsText}
          disabled={!loaded}
          rows={5}
          placeholder={'One per line or comma-separated, e.g.\nAcme Cloud\nPriya Raman\nPlatform Team'}
          onChange={(e) => setHotwordsText(e.target.value)}
        />
        <p className="text-xs text-muted-foreground">
          Product, customer and team names. They bias recognition toward these spellings but don’t guarantee them.
        </p>
      </div>

      <Button onClick={handleSave} disabled={!loaded || saving}>
        {saving ? 'Saving…' : 'Save VibeVoice settings'}
      </Button>
    </div>
  );
}
