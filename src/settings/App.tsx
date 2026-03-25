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

const GEMINI_MODELS = [
  {
    value: "gemini-2.5-flash-lite",
    label: "Gemini 2.5 Flash-Lite"
  },
  {
    value: "gemini-2.5-flash",
    label: "Gemini 2.5 Flash"
  },
];

const TTS_VOICES = [
  { value: "en-US-Standard-A", label: "Standard-A (Male)" },
  { value: "en-US-Standard-B", label: "Standard-B (Male)" },
  { value: "en-US-Standard-C", label: "Standard-C (Female)" },
  { value: "en-US-Standard-D", label: "Standard-D (Male)" },
  { value: "en-US-Standard-E", label: "Standard-E (Female)" },
  { value: "en-US-Standard-F", label: "Standard-F (Female)" },
  { value: "en-US-Standard-G", label: "Standard-G (Female)" },
  { value: "en-US-Standard-H", label: "Standard-H (Female)" },
  { value: "en-US-Standard-I", label: "Standard-I (Male)" },
  { value: "en-US-Standard-J", label: "Standard-J (Male)" },
];

async function resizeToContent() {
  await new Promise((r) => requestAnimationFrame(r));
  const height = document.body.scrollHeight;
  await getCurrentWindow().setSize(new LogicalSize(400, height));
}

export function App() {
  const [googleKey, setGoogleKey] = useState("");
  const [geminiModel, setGeminiModel] = useState("gemini-2.5-flash");
  const [additionalPrompt, setAdditionalPrompt] = useState("");
  const [autoStart, setAutoStart] = useState(false);
  const [ttsVoice, setTtsVoice] = useState("en-US-Standard-C");
  const [ttsSpeed, setTtsSpeed] = useState(1.0);
  const [playing, setPlaying] = useState(false);

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
    invoke<AppConfig>("get_config").then((config) => {
      setGoogleKey(config.google_api_key);
      setGeminiModel(config.gemini_model);
      setAdditionalPrompt(config.additional_prompt);
      setAutoStart(config.auto_start);
      setTtsVoice(config.tts_voice);
      setTtsSpeed(config.tts_speed);
    });
  }, []);

  useEffect(() => {
    resizeToContent();
  });

  const handleSave = useCallback(async () => {
    const config: AppConfig = {
      google_api_key: googleKey,
      additional_prompt: additionalPrompt,
      auto_start: autoStart,
      gemini_model: geminiModel,
      tts_voice: ttsVoice,
      tts_speed: ttsSpeed,
    };
    await invoke("save_config", { config });
    await getCurrentWindow().close();
  }, [googleKey, geminiModel, additionalPrompt, autoStart, ttsVoice, ttsSpeed]);

  const handleCancel = useCallback(async () => {
    await getCurrentWindow().close();
  }, []);

  return (
    <div className="content">
      <div className="field">
        <label htmlFor="google-key">Google API Key</label>
        <input
          type="password"
          id="google-key"
          placeholder="AIza..."
          value={googleKey}
          onChange={(e) => setGoogleKey(e.target.value)}
        />
      </div>
      <div className="field">
        <label htmlFor="gemini-model">Gemini Model (Input / Output per 1M tokens)</label>
        <select
          id="gemini-model"
          value={geminiModel}
          onChange={(e) => setGeminiModel(e.target.value)}
        >
          {GEMINI_MODELS.map((m) => (
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
          value={additionalPrompt}
          onChange={(e) => setAdditionalPrompt(e.target.value)}
        />
      </div>
      <div className="field">
        <label htmlFor="tts-voice">Voice Type</label>
        <div className="voice">
          <select
            id="tts-voice"
            value={ttsVoice}
            onChange={(e) => setTtsVoice(e.target.value)}
          >
            {TTS_VOICES.map((v) => (
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
                : invoke("preview_tts", { voice: ttsVoice, speed: ttsSpeed })
            }
          >
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 -960 960 960">
              <path d="M232-81q57 0 99-31t62-82q20-51 39.5-79.5T506-345q66-53 95-113t29-146q0-120-76-196.5T358-877q-118 0-195.5 73.5T80-616h60q5-88 65.5-144.5T358-817q90 0 151 61.5T570-604q0 72-28 124.5T449-378q-39 29-62.5 63T342-231q-17 42-44.5 66T232-141q-35 0-60.5-24T141-224H81q5 60 48 101.5T232-81Zm192-457q27-27 27-66t-27-67q-27-28-66-28t-67 28q-28 28-28 67t28 66q28 27 67 27t66-27Zm323 151-47-46q20-39 30-82.5t10-90.5q0-47-10-90.5T701-779l46-46q26 49 39.5 103.5T800-607q0 60-13.5 115T747-387Zm117 114-45-43q38-63 59.5-136T900-604q0-80-21.5-153.5T818-894l45-44q47 72 72 156.5T960-604q0 92-25 175.5T864-273Z" />
            </svg>
          </button>
        </div>
      </div>
      <div className="field">
        <label htmlFor="tts-speed">Voice Speed ({ttsSpeed.toFixed(1)}x)</label>
        <input
          type="range"
          id="tts-speed"
          min="0.5"
          max="2.0"
          step="0.1"
          value={ttsSpeed}
          onChange={(e) => setTtsSpeed(parseFloat(e.target.value))}
        />
      </div>
      <div className="field checkbox-field">
        <label>
          <input
            type="checkbox"
            checked={autoStart}
            onChange={(e) => setAutoStart(e.target.checked)}
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
