import { useEffect, useState, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

interface TranslationResult {
  translated: string;
  grammar: string;
  improvements: string;
  source_is_english: boolean;
}

type ViewState =
  | { type: "loading" }
  | { type: "result"; result: TranslationResult }
  | { type: "error"; message: string };

const POPUP_WIDTH = 520;
const MIN_HEIGHT = 300;
const MAX_HEIGHT = 600;

async function resizeToContent() {
  await new Promise((r) => requestAnimationFrame(r));
  const height = Math.min(
    Math.max(document.body.scrollHeight, MIN_HEIGHT),
    MAX_HEIGHT
  );
  await getCurrentWindow().setSize(new LogicalSize(POPUP_WIDTH, height));
}

export function App() {
  const [view, setView] = useState<ViewState>({ type: "loading" });
  const [originalText, setOriginalText] = useState("");
  const [playing, setPlaying] = useState(false);
  const resultRef = useRef<TranslationResult | null>(null);

  if (view.type === "result") {
    resultRef.current = view.result;
  }

  useEffect(() => {
    const unlisten = [
      listen<string>("show-loading", (e) => {
        setOriginalText(e.payload || "");
        setView({ type: "loading" });
      }),
      listen<TranslationResult>("show-result", (e) => {
        setView({ type: "result", result: e.payload });
      }),
      listen<string>("show-error", (e) => {
        setView({ type: "error", message: e.payload });
      }),
      getCurrentWindow().listen("tts-playing", () => setPlaying(true)),
      getCurrentWindow().listen("tts-done", () => setPlaying(false)),
    ];
    return () => {
      unlisten.forEach((u) => u.then((f) => f()));
    };
  }, []);

  useEffect(() => {
    if (view.type !== "loading") {
      resizeToContent();
    }
  }, [view]);

  const closePopup = useCallback(async () => {
    await getCurrentWindow().hide();
  }, []);

  const handleReplace = useCallback(async () => {
    if (resultRef.current) {
      await invoke("do_paste", { text: resultRef.current.translated });
    }
    await closePopup();
  }, [closePopup]);

  const handleClose = useCallback(async () => {
    await invoke("restore_original_text");
    await closePopup();
  }, [closePopup]);

  const handlePlay = useCallback(async () => {
    if (playing) {
      await invoke("stop_tts");
    } else if (resultRef.current) {
      await invoke("play_tts", { text: resultRef.current.translated });
    }
  }, [playing]);

  const handleRetry = useCallback(async () => {
    await invoke("retry_translation");
  }, []);

  return (
    <div id="popup">
      <div className="drag-handle" data-tauri-drag-region />
      <button className="btn-close" title="Close" onClick={handleClose}>
        &#x2715;
      </button>
      {view.type === "loading" && (
        <div className="loading">
          <div className="spinner" />
          <span>Translating...</span>
        </div>
      )}
      {view.type === "result" && (
        <div className="content">
          <div className="section">
            <div className="section-title">元テキスト</div>
            <div className="original-text">{originalText}</div>
          </div>
          <div className="section">
            <div className="section-title">推奨英文</div>
            <div className="translated-text">{view.result.translated}</div>
          </div>
          <div className="section">
            <div className="section-title">文法解説</div>
            <div className="grammar">{view.result.grammar}</div>
          </div>
          {view.result.source_is_english &&
            view.result.improvements &&
            view.result.improvements !== "N/A" && (
              <div className="section">
                <div className="section-title">改善点</div>
                <div className="improvements">{view.result.improvements}</div>
              </div>
            )}
          <div className="buttons">
            <button className="btn-primary" onClick={handleReplace}>
              Replace
            </button>
            <button onClick={handlePlay} aria-pressed={playing || undefined}>
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 -960 960 960"><path d="M232-81q57 0 99-31t62-82q20-51 39.5-79.5T506-345q66-53 95-113t29-146q0-120-76-196.5T358-877q-118 0-195.5 73.5T80-616h60q5-88 65.5-144.5T358-817q90 0 151 61.5T570-604q0 72-28 124.5T449-378q-39 29-62.5 63T342-231q-17 42-44.5 66T232-141q-35 0-60.5-24T141-224H81q5 60 48 101.5T232-81Zm192-457q27-27 27-66t-27-67q-27-28-66-28t-67 28q-28 28-28 67t28 66q28 27 67 27t66-27Zm323 151-47-46q20-39 30-82.5t10-90.5q0-47-10-90.5T701-779l46-46q26 49 39.5 103.5T800-607q0 60-13.5 115T747-387Zm117 114-45-43q38-63 59.5-136T900-604q0-80-21.5-153.5T818-894l45-44q47 72 72 156.5T960-604q0 92-25 175.5T864-273Z"/></svg>
            </button>
          </div>
        </div>
      )}
      {view.type === "error" && (
        <div className="content">
          <p className="error-msg">{view.message}</p>
          <div className="buttons">
            <button className="btn-primary" onClick={handleRetry}>
              Retry
            </button>
            <button onClick={handleClose}>
              Close
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
