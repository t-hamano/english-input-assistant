import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

interface AppConfig {
  google_api_key: string;
  additional_prompt: string;
  auto_start: boolean;
  gemini_model: string;
  tts_voice: string;
  tts_speed: number;
}

interface GeminiModel {
  value: string;
  label: string;
  is_default: boolean;
}

interface TtsVoice {
  value: string;
  label: string;
  is_default: boolean;
}

async function resizeToContent() {
  await new Promise((r) => requestAnimationFrame(r));
  const height = document.body.scrollHeight;
  await getCurrentWindow().setSize(new LogicalSize(400, height));
}

export function App() {
  const [settings, setSettings] = useState<AppConfig | null>(null);
  const [geminiModels, setGeminiModels] = useState<GeminiModel[]>([]);
  const [ttsVoices, setTtsVoices] = useState<TtsVoice[]>([]);
  const [playing, setPlaying] = useState(false);

  const update = useCallback(
    <K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
      setSettings((prev) => prev && ({ ...prev, [key]: value }));
    },
    []
  );

  useEffect(() => {
    const unlisten = [
      getCurrentWindow().listen("tts-playing", () => setPlaying(true)),
      getCurrentWindow().listen("tts-done", () => setPlaying(false)),
    ];
    return () => {
      unlisten.forEach((u) => u.then((f) => f()));
    };
  }, []);

  useEffect(() => {
    Promise.all([
      invoke<GeminiModel[]>("get_gemini_models"),
      invoke<TtsVoice[]>("get_tts_voices"),
      invoke<AppConfig>("get_config"),
    ]).then(([models, voices, config]) => {
      setGeminiModels(models);
      setTtsVoices(voices);
      setSettings(config);
    });
  }, []);

  useEffect(() => {
    resizeToContent();
  });

  const handleSave = useCallback(async () => {
    if (!settings) return;
    await invoke("save_config", { config: settings });
    await getCurrentWindow().close();
  }, [settings]);

  const handleCancel = useCallback(async () => {
    await getCurrentWindow().close();
  }, []);

  if (!settings) return null;

  return (
    <div className="content">
      <div className="field">
        <label htmlFor="google-key">Google API Key</label>
        <input
          type="password"
          id="google-key"
          placeholder="AIza..."
          value={settings.google_api_key}
          onChange={(e) => update("google_api_key", e.target.value)}
        />
      </div>
      <div className="field">
        <label htmlFor="gemini-model">Gemini Model</label>
        <select
          id="gemini-model"
          value={settings.gemini_model}
          onChange={(e) => update("gemini_model", e.target.value)}
        >
          {geminiModels.map((m) => (
            <option key={m.value} value={m.value}>
              {m.label}
            </option>
          ))}
        </select>
      </div>
      <div className="field">
        <label htmlFor="additional-prompt">Additional Prompt</label>
        <textarea
          id="additional-prompt"
          rows={3}
          placeholder="e.g. Use casual tone"
          value={settings.additional_prompt}
          onChange={(e) => update("additional_prompt", e.target.value)}
        />
      </div>
      <div className="field">
        <label htmlFor="tts-voice">Voice Type</label>
        <div className="voice">
          <select
            id="tts-voice"
            value={settings.tts_voice}
            onChange={(e) => update("tts_voice", e.target.value)}
          >
            {ttsVoices.map((v) => (
              <option key={v.value} value={v.value}>
                {v.label}
              </option>
            ))}
          </select>
          <button
            type="button"
            aria-label={playing ? "Stop preview" : "Play preview"}
            aria-pressed={playing || undefined}
            onClick={() =>
              playing
                ? invoke("stop_tts")
                : invoke("preview_tts", { voice: settings.tts_voice, speed: settings.tts_speed })
            }
          >
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 -960 960 960">
              <path d="M232-81q57 0 99-31t62-82q20-51 39.5-79.5T506-345q66-53 95-113t29-146q0-120-76-196.5T358-877q-118 0-195.5 73.5T80-616h60q5-88 65.5-144.5T358-817q90 0 151 61.5T570-604q0 72-28 124.5T449-378q-39 29-62.5 63T342-231q-17 42-44.5 66T232-141q-35 0-60.5-24T141-224H81q5 60 48 101.5T232-81Zm192-457q27-27 27-66t-27-67q-27-28-66-28t-67 28q-28 28-28 67t28 66q28 27 67 27t66-27Zm323 151-47-46q20-39 30-82.5t10-90.5q0-47-10-90.5T701-779l46-46q26 49 39.5 103.5T800-607q0 60-13.5 115T747-387Zm117 114-45-43q38-63 59.5-136T900-604q0-80-21.5-153.5T818-894l45-44q47 72 72 156.5T960-604q0 92-25 175.5T864-273Z" />
            </svg>
          </button>
        </div>
      </div>
      <div className="field">
        <label htmlFor="tts-speed">Voice Speed ({settings.tts_speed.toFixed(1)}x)</label>
        <input
          type="range"
          id="tts-speed"
          min="0.5"
          max="2.0"
          step="0.1"
          value={settings.tts_speed}
          onChange={(e) => update("tts_speed", parseFloat(e.target.value))}
        />
      </div>
      <div className="field checkbox-field">
        <label>
          <input
            type="checkbox"
            checked={settings.auto_start}
            onChange={(e) => update("auto_start", e.target.checked)}
          />
          Launch at startup
        </label>
      </div>
      <div className="buttons">
        <button className="btn-primary" onClick={handleSave}>
          Save
        </button>
        <button onClick={handleCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}
