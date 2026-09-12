import { getVersion } from "@tauri-apps/api/app";
import { Channel, invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import { PlayIcon } from "../icons";

interface AppConfig {
  google_api_key: string;
  additional_prompt: string;
  auto_start: boolean;
  auto_check_updates: boolean;
  gemini_model: string;
  tts_voice: string;
  tts_speed: number;
  shortcut: string;
}

const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
  "OSLeft",
  "OSRight",
]);

function formatKeyName(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (code.startsWith("Numpad")) return `Num${code.slice(6)}`;
  if (code.startsWith("Arrow")) return code.slice(5);
  return code;
}

function formatShortcut(s: string): string {
  if (!s) return "";
  const parts = s.split("+");
  // biome-ignore lint/style/noNonNullAssertion: split() on a non-empty string always yields at least one part
  const last = parts.pop()!;
  return [...parts, formatKeyName(last)].join("+");
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

type UpdateOutcome = "up_to_date" | "cancelled" | "busy" | "development";
type UpdateProgress =
  | { event: "downloading"; received: number; total: number | null }
  | { event: "installing" };

const IS_DEV = import.meta.env.DEV;
const DEV_UPDATE_MESSAGE = "アップデートはインストール済みのリリースビルドでのみ利用できます。";

export function App() {
  const [settings, setSettings] = useState<AppConfig | null>(null);
  const [geminiModels, setGeminiModels] = useState<GeminiModel[]>([]);
  const [ttsVoices, setTtsVoices] = useState<TtsVoice[]>([]);
  const [playing, setPlaying] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState("");
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [updateStatus, setUpdateStatus] = useState("");
  const [updateError, setUpdateError] = useState(false);

  const update = useCallback(<K extends keyof AppConfig>(key: K, value: AppConfig[K]) => {
    setSettings((prev) => prev && { ...prev, [key]: value });
  }, []);

  useEffect(() => {
    const unlisten = [
      getCurrentWindow().listen("tts-playing", () => setPlaying(true)),
      getCurrentWindow().listen("tts-done", () => setPlaying(false)),
      getCurrentWindow().listen<string>("tts-error", (e) => setError(e.payload)),
    ];
    return () => {
      unlisten.forEach((u) => {
        u.then((f) => f());
      });
    };
  }, []);

  useEffect(() => {
    void getVersion().then(setVersion).catch(console.error);
    Promise.all([
      invoke<GeminiModel[]>("get_gemini_models"),
      invoke<TtsVoice[]>("get_tts_voices"),
      invoke<AppConfig>("get_config"),
    ])
      .then(([models, voices, config]) => {
        setGeminiModels(models);
        setTtsVoices(voices);
        setSettings(config);
      })
      .catch((e) => {
        setError(String(e));
      });
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: updateStatus isn't read here, but re-measures scrollHeight after its text changes the layout
  useLayoutEffect(() => {
    if (!settings && !error) return;
    const height = Math.max(300, document.body.scrollHeight);
    void getCurrentWindow().setSize(new LogicalSize(500, height)).catch(console.error);
  }, [settings, error, updateStatus]);

  const handleCheckForUpdates = useCallback(async () => {
    setCheckingUpdates(true);
    setUpdateError(false);
    setUpdateStatus("アップデートを確認中...");
    const onProgress = new Channel<UpdateProgress>();
    onProgress.onmessage = (progress) => {
      if (progress.event === "installing") {
        setUpdateStatus("アップデートをインストール中。アプリが再起動します...");
      } else if (progress.total) {
        const percent = Math.min(100, Math.round((progress.received / progress.total) * 100));
        setUpdateStatus(`アップデートをダウンロード中... ${percent}%`);
      } else {
        setUpdateStatus("アップデートをダウンロード中...");
      }
    };
    try {
      const result = await invoke<UpdateOutcome>("check_for_updates", { onProgress });
      const messages: Record<UpdateOutcome, string> = {
        up_to_date: "最新バージョンを使用しています。",
        cancelled: "アップデートを延期しました。後で再確認できます。",
        busy: "アップデートの確認がすでに進行中です。",
        development: DEV_UPDATE_MESSAGE,
      };
      setUpdateStatus(messages[result]);
    } catch (e) {
      setUpdateError(true);
      setUpdateStatus(String(e));
    } finally {
      setCheckingUpdates(false);
    }
  }, []);

  const handleSave = useCallback(async () => {
    if (!settings) return;
    try {
      await invoke("save_config", { config: settings });
      await getCurrentWindow().close();
    } catch (e) {
      setError(String(e));
    }
  }, [settings]);

  const handleShortcutKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.code === "Escape") {
        setCapturing(false);
        e.currentTarget.blur();
        return;
      }

      if (MODIFIER_CODES.has(e.code)) return;

      const parts: string[] = [];
      if (e.ctrlKey) parts.push("Ctrl");
      if (e.altKey) parts.push("Alt");
      if (e.shiftKey) parts.push("Shift");
      if (e.metaKey) parts.push("Meta");
      if (parts.length === 0) return;
      parts.push(e.code);

      update("shortcut", parts.join("+"));
      setError(null);
      setCapturing(false);
      e.currentTarget.blur();
    },
    [update],
  );

  const handleCancel = useCallback(async () => {
    await getCurrentWindow().close();
  }, []);

  if (!settings) {
    return (
      <div className="content">
        {error ? (
          <div className="error" role="alert">
            {error}
          </div>
        ) : (
          <p>設定を読み込み中...</p>
        )}
      </div>
    );
  }

  return (
    <div className="content">
      <div className="field">
        <label htmlFor="shortcut">ショートカットキー</label>
        <div className="shortcut">
          <input
            type="text"
            id="shortcut"
            readOnly
            className="shortcut-input"
            value={capturing ? "キーを押してください..." : formatShortcut(settings.shortcut)}
            placeholder="クリックして記録"
            onFocus={() => setCapturing(true)}
            onBlur={() => setCapturing(false)}
            onKeyDown={handleShortcutKeyDown}
          />
          <button
            type="button"
            onClick={() => {
              update("shortcut", "");
              setError(null);
            }}
            disabled={!settings.shortcut}
          >
            リセット
          </button>
        </div>
      </div>
      <div className="field">
        <label htmlFor="google-key">Google API キー</label>
        <input
          type="password"
          id="google-key"
          placeholder="AIza..."
          value={settings.google_api_key}
          onChange={(e) => update("google_api_key", e.target.value)}
        />
        <small>この端末に安全に保存されます。削除するには空欄にして保存してください。</small>
      </div>
      <div className="field">
        <label htmlFor="gemini-model">Gemini モデル</label>
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
        <small>Flash は解説の精度が高いが低速、Flash-Lite は低コスト・高速で短文向けです。</small>
      </div>
      <div className="field">
        <label htmlFor="additional-prompt">追加プロンプト</label>
        <textarea
          id="additional-prompt"
          rows={3}
          placeholder="例: ビジネスメール向けにフォーマルな表現で。1文は簡潔に、専門用語はそのまま残す。"
          value={settings.additional_prompt}
          onChange={(e) => update("additional_prompt", e.target.value)}
        />
      </div>
      <div className="field">
        <label htmlFor="tts-voice">音声タイプ</label>
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
            style={{ width: 112 }}
            aria-label={playing ? "プレビュー停止" : "プレビュー再生"}
            aria-pressed={playing || undefined}
            onClick={() =>
              playing
                ? invoke("stop_tts")
                : invoke("preview_tts", { voice: settings.tts_voice, speed: settings.tts_speed })
            }
          >
            <PlayIcon />
            <span>{playing ? "停止" : "プレビュー"}</span>
          </button>
        </div>
        <small>
          Standard は低コストで無料枠が大きめ、Chirp 3 HD
          はより自然で高品質ですが単価が高く無料枠は小さめです。
        </small>
      </div>
      <div className="field">
        <label htmlFor="tts-speed">音声速度 ({settings.tts_speed.toFixed(1)}x)</label>
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
          起動時に自動実行
        </label>
      </div>
      <div className="field checkbox-field">
        <label>
          <input
            type="checkbox"
            checked={settings.auto_check_updates}
            onChange={(e) => update("auto_check_updates", e.target.checked)}
          />
          起動時にアップデートを確認
        </label>
      </div>
      <div className="field updates">
        <div className="updates-heading">
          <span>バージョン {version || "..."}</span>
          <button
            type="button"
            onClick={handleCheckForUpdates}
            disabled={checkingUpdates || IS_DEV}
          >
            {checkingUpdates ? "更新中..." : "アップデートを確認"}
          </button>
        </div>
        {IS_DEV ? (
          <p className="update-status" role="status">
            {DEV_UPDATE_MESSAGE}
          </p>
        ) : (
          updateStatus && (
            <p
              className={updateError ? "error" : "update-status"}
              role={updateError ? "alert" : "status"}
            >
              {updateStatus}
            </p>
          )
        )}
      </div>
      {error && <div className="error">{error}</div>}
      <div className="buttons">
        <button type="button" className="btn-primary" onClick={handleSave}>
          保存
        </button>
        <button type="button" onClick={handleCancel}>
          キャンセル
        </button>
      </div>
    </div>
  );
}
