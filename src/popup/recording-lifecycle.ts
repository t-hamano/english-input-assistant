import type { Window } from "@tauri-apps/api/window";

// Backend capture can hide the popup and exit without sending a translation
// event. Stop the microphone independently of translation success or failure.
export function watchRecordingLifecycle(
  popup: Pick<Window, "listen" | "onFocusChanged">,
  resetRecording: () => void,
): () => void {
  let disposed = false;
  const cleanups: (() => void)[] = [];
  const register = (subscription: Promise<() => void>) => {
    void subscription.then((unlisten) => {
      if (disposed) unlisten();
      else cleanups.push(unlisten);
    }).catch(() => resetRecording());
  };
  register(popup.listen("popup-hiding", resetRecording));
  register(popup.onFocusChanged(({ payload: focused }) => {
    if (!focused) resetRecording();
  }));
  return () => {
    disposed = true;
    resetRecording();
    cleanups.forEach((unlisten) => unlisten());
  };
}
