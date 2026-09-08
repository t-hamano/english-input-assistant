import { useEffect, useState, useCallback, useRef, useReducer } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

import { initialTranslationState, translationReducer } from "./translation-state";
import type { TranslationResult } from "./translation-state";

const POPUP_WIDTH = 520;
const MIN_HEIGHT = 300;
const MAX_HEIGHT = 600;

async function resizeToContent() {
  await new Promise((r) => requestAnimationFrame(r));
  const content = document.querySelector<HTMLElement>("#popup > .content");
  const height = Math.min(
    Math.max(content?.scrollHeight ?? document.body.scrollHeight, MIN_HEIGHT),
    MAX_HEIGHT
  );
  await getCurrentWindow().setSize(new LogicalSize(POPUP_WIDTH, height));
}

export function App() {
  const [translation, dispatch] = useReducer(translationReducer, initialTranslationState);
  const { view, originalText } = translation;
  const [playing, setPlaying] = useState(false);
  const [audioError, setAudioError] = useState<string | null>(null);
  const [recording, setRecording] = useState(false);
  const [recordingPlaying, setRecordingPlaying] = useState(false);
  const resultRef = useRef<TranslationResult | null>(null);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const recordingRequestRef = useRef(0);
  const recordingPendingRef = useRef(false);
  const recordedAudioRef = useRef<HTMLAudioElement | null>(null);

  resultRef.current = translation.requestId !== null && view.type === "result" ? view.result : null;

  const resetRecording = useCallback(() => {
    // Invalidate pending microphone requests and queued recorder events.
    recordingRequestRef.current += 1;
    recordingPendingRef.current = false;
    const recorder = mediaRecorderRef.current;
    mediaRecorderRef.current = null;
    if (recorder) {
      recorder.ondataavailable = null;
      recorder.onstop = null;
      try {
        if (recorder.state !== "inactive") recorder.stop();
      } finally {
        recorder.stream.getTracks().forEach((track) => track.stop());
      }
    }
    if (recordedAudioRef.current) {
      recordedAudioRef.current.pause();
      URL.revokeObjectURL(recordedAudioRef.current.src);
      recordedAudioRef.current = null;
    }
    setRecording(false);
    setRecordingPlaying(false);
  }, []);

  useEffect(() => {
    const unlisten = [
      listen<{ request_id: number; text: string }>("show-loading", (e) => {
        resultRef.current = null;
        dispatch({ type: "start", ...e.payload });
        resetRecording();
      }),
      listen<{ request_id: number; result: TranslationResult; complete: boolean }>("translation-update", (e) => {
        dispatch({ type: "update", ...e.payload });
      }),
      listen<{ request_id: number; message: string }>("translation-error", (e) => {
        dispatch({ type: "error", ...e.payload });
      }),
      listen<string>("show-error", (e) => {
        resetRecording();
        resultRef.current = null;
        dispatch({ type: "preflight-error", message: e.payload });
      }),
      getCurrentWindow().listen("tts-playing", () => { setPlaying(true); setAudioError(null); }),
      getCurrentWindow().listen("tts-done", () => setPlaying(false)),
      getCurrentWindow().listen<string>("tts-error", (e) => setAudioError(e.payload)),
    ];
    return () => {
      resetRecording();
      unlisten.forEach((u) => u.then((f) => f()));
    };
  }, [resetRecording]);

  useEffect(() => {
    if (view.type === "loading") return;
    const content = document.querySelector<HTMLElement>("#popup > .content");
    if (!content) return;
    // Re-measure after width/DPI changes reflow the text, not just after API events.
    const observer = new ResizeObserver(() => { void resizeToContent(); });
    observer.observe(content);
    return () => observer.disconnect();
  }, [view.type]);

  const closePopup = useCallback(async () => {
    await getCurrentWindow().hide();
  }, []);

  const handleReplace = useCallback(async () => {
    const result = resultRef.current;
    if (!result) return;
    try {
      await invoke("do_paste", { text: result.translated });
    } catch {
      // Paste target unconfirmed; backend re-showed the popup. Keep it open.
      return;
    }
    resetRecording();
    dispatch({ type: "dismiss" });
    resultRef.current = null;
    await closePopup();
  }, [closePopup, resetRecording]);

  const handleClose = useCallback(async () => {
    resetRecording();
    dispatch({ type: "dismiss" });
    resultRef.current = null;
    await invoke("restore_original_text");
    await closePopup();
  }, [closePopup, resetRecording]);

  const handlePlay = useCallback(async () => {
    if (playing) {
      await invoke("stop_tts");
    } else if (resultRef.current) {
      await invoke("play_tts", { text: resultRef.current.translated });
    }
  }, [playing]);

  const handleRetry = useCallback(async () => {
    resetRecording();
    dispatch({ type: "retry" });
    resultRef.current = null;
    await invoke("retry_translation");
  }, [resetRecording]);

  const handleRecord = useCallback(async () => {
    const activeRecorder = mediaRecorderRef.current;
    if (activeRecorder) {
      try {
        if (activeRecorder.state !== "inactive") activeRecorder.stop();
      } finally {
        activeRecorder.stream.getTracks().forEach((track) => track.stop());
      }
      return;
    }
    if (recordingPendingRef.current) return;
    recordingPendingRef.current = true;
    const requestId = ++recordingRequestRef.current;
    let stream: MediaStream | undefined;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      if (requestId !== recordingRequestRef.current) {
        stream.getTracks().forEach((track) => track.stop());
        return;
      }
      const recorder = new MediaRecorder(stream);
      const chunks: Blob[] = [];
      mediaRecorderRef.current = recorder;
      recorder.ondataavailable = (e) => {
        if (requestId === recordingRequestRef.current && e.data.size > 0) chunks.push(e.data);
      };
      recorder.onstop = () => {
        recorder.stream.getTracks().forEach((track) => track.stop());
        if (requestId !== recordingRequestRef.current) return;
        mediaRecorderRef.current = null;
        const blob = new Blob(chunks, { type: recorder.mimeType });
        const url = URL.createObjectURL(blob);
        if (recordedAudioRef.current) {
          URL.revokeObjectURL(recordedAudioRef.current.src);
        }
        recordedAudioRef.current = new Audio(url);
        setRecording(false);
        resizeToContent();
      };
      recorder.start();
      setRecording(true);
      resizeToContent();
    } catch {
      // Release an acquired microphone even if recorder construction/start fails.
      stream?.getTracks().forEach((track) => track.stop());
      if (requestId === recordingRequestRef.current) resetRecording();
    } finally {
      if (requestId === recordingRequestRef.current) recordingPendingRef.current = false;
    }
  }, [resetRecording]);

  const handlePlayRecording = useCallback(() => {
    if (recordingPlaying) {
      recordedAudioRef.current?.pause();
      if (recordedAudioRef.current) recordedAudioRef.current.currentTime = 0;
      setRecordingPlaying(false);
      return;
    }
    const audio = recordedAudioRef.current;
    if (!audio) return;
    audio.onended = () => setRecordingPlaying(false);
    audio.play();
    setRecordingPlaying(true);
  }, [recordingPlaying]);

  return (
    <div id="popup">
      <div className="drag-handle" data-tauri-drag-region>
        <span className="popup-title">
          English Input Assistant{view.type === "result" ? "：翻訳結果" : view.type === "error" ? "：エラー" : "：翻訳中"}
        </span>
      </div>
      <button className="popup-close" title="閉じる" onClick={handleClose}>
        &#x2715;
      </button>
      {view.type === "loading" && (
        <div className="loading">
          <div className="spinner" />
          <span>翻訳中...</span>
        </div>
      )}
      {view.type === "result" && (
        <div className="content">
          <div className="section">
            <div className="section-title">原文</div>
            <div className="original-text">{originalText}</div>
          </div>
          <div className="section">
            <div className="section-title">推奨英文</div>
            <div className="translated-text">{view.result.translated}</div>
          </div>
          <div className="section">
            <div className="audio-buttons">
              <button
                aria-label={playing ? "停止" : "再生"}
                aria-pressed={playing || undefined}
                onClick={handlePlay}
              >
                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 -960 960 960"><path d="M232-81q57 0 99-31t62-82q20-51 39.5-79.5T506-345q66-53 95-113t29-146q0-120-76-196.5T358-877q-118 0-195.5 73.5T80-616h60q5-88 65.5-144.5T358-817q90 0 151 61.5T570-604q0 72-28 124.5T449-378q-39 29-62.5 63T342-231q-17 42-44.5 66T232-141q-35 0-60.5-24T141-224H81q5 60 48 101.5T232-81Zm192-457q27-27 27-66t-27-67q-27-28-66-28t-67 28q-28 28-28 67t28 66q28 27 67 27t66-27Zm323 151-47-46q20-39 30-82.5t10-90.5q0-47-10-90.5T701-779l46-46q26 49 39.5 103.5T800-607q0 60-13.5 115T747-387Zm117 114-45-43q38-63 59.5-136T900-604q0-80-21.5-153.5T818-894l45-44q47 72 72 156.5T960-604q0 92-25 175.5T864-273Z"/></svg>
                <span>{playing ? "停止" : "再生"}</span>
              </button>
              <button
                aria-label={recording ? "録音停止" : "録音"}
                aria-pressed={recording || undefined}
                disabled={recordingPlaying}
                onClick={handleRecord}
              >
                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 -960 960 960"><path d="M395-435q-35-35-35-85v-240q0-50 35-85t85-35q50 0 85 35t35 85v240q0 50-35 85t-85 35q-50 0-85-35Zm85-205Zm-40 520v-123q-104-14-172-93t-68-184h80q0 83 58.5 141.5T480-320q83 0 141.5-58.5T680-520h80q0 105-68 184t-172 93v123h-80Zm68.5-371.5Q520-503 520-520v-240q0-17-11.5-28.5T480-800q-17 0-28.5 11.5T440-760v240q0 17 11.5 28.5T480-480q17 0 28.5-11.5Z"/></svg>
                <span>{recording ? "録音停止" : "録音"}</span>
              </button>
              <button
                aria-label={recordingPlaying ? "再生停止" : "録音再生"}
                aria-pressed={recordingPlaying || undefined}
                disabled={recording || !recordedAudioRef.current}
                onClick={handlePlayRecording}
              >
                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 -960 960 960"><path d="M320-200v-560l440 280-440 280Zm80-280Zm0 134 210-134-210-134v268Z"/></svg>
                <span>{recordingPlaying ? "再生停止" : "録音再生"}</span>
              </button>
            </div>
          </div>
          <div className="section">
            <div className="section-title">{view.result.source_is_english ? "改善点・文法解説" : "文法解説"}</div>
            <div className="explanation" key={translation.latestRequestId} aria-busy={!view.complete} role="region" aria-label={view.result.source_is_english ? "改善点・文法解説" : "文法解説"} tabIndex={0}>
              {view.error ? (
                <div role="alert">
                  <p className="error-msg">解説を読み込めませんでした。英文はそのまま使用できます。</p>
                  <details><summary>エラー詳細</summary>{view.error}</details>
                  <button onClick={handleRetry}>翻訳を再試行</button>
                </div>
              ) : !view.complete ? (
                <span role="status">解説を読み込み中...</span>
              ) : view.result.explanation}
            </div>
          </div>
          {audioError && <p className="error-msg" role="alert">{audioError}</p>}
          <button className="btn-primary btn-replace" onClick={handleReplace}>
            置き換え
          </button>
        </div>
      )}
      {view.type === "error" && (
        <div className="content">
          <p className="error-msg">{view.message}</p>
          <div className="buttons">
            <button className="btn-primary" onClick={handleRetry}>
              再試行
            </button>
            <button onClick={handleClose}>
              閉じる
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
