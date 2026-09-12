import assert from "node:assert/strict";
import { test } from "node:test";
import type { Window } from "@tauri-apps/api/window";
import { watchRecordingLifecycle } from "../src/popup/recording-lifecycle.ts";

type LifecyclePopup = Pick<Window, "listen" | "onFocusChanged">;

function popupFixture() {
  const listeners = new Map<string, (event: { payload: boolean }) => void>();
  const subscribe = async (event: string, callback: (event: { payload: boolean }) => void) => {
    listeners.set(event, callback);
    return () => {
      listeners.delete(event);
    };
  };
  const popup = {
    listen: subscribe,
    onFocusChanged: (callback: (event: { payload: boolean }) => void) =>
      subscribe("focus", callback),
  } as unknown as LifecyclePopup;
  return { popup, listeners };
}

test("backend hide stops recording even when capture emits no loading/error event", async () => {
  const { popup, listeners } = popupFixture();
  let microphoneActive = true;
  let pendingPermissionRequest = 1;
  const dispose = watchRecordingLifecycle(popup, () => {
    microphoneActive = false;
    pendingPermissionRequest++;
  });
  await Promise.resolve();
  // biome-ignore lint/style/noNonNullAssertion: registered synchronously above by popupFixture
  listeners.get("popup-hiding")!({ payload: false });
  assert.equal(microphoneActive, false);
  assert.equal(pendingPermissionRequest, 2);
  dispose();
  assert.equal(listeners.size, 0);
});

test("losing focus stops the microphone without a translation event", async () => {
  const { popup, listeners } = popupFixture();
  let stops = 0;
  const dispose = watchRecordingLifecycle(popup, () => {
    stops++;
  });
  await Promise.resolve();
  // biome-ignore lint/style/noNonNullAssertion: registered synchronously above by popupFixture
  listeners.get("focus")!({ payload: true });
  assert.equal(stops, 0);
  // biome-ignore lint/style/noNonNullAssertion: registered synchronously above by popupFixture
  listeners.get("focus")!({ payload: false });
  assert.equal(stops, 1);
  dispose();
});

test("subscriptions completing after unmount are still removed", async () => {
  const { popup, listeners } = popupFixture();
  let stops = 0;
  const dispose = watchRecordingLifecycle(popup, () => {
    stops++;
  });
  dispose();
  await Promise.resolve();
  assert.equal(stops, 1);
  assert.equal(listeners.size, 0);
});
