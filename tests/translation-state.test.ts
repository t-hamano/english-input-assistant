import assert from "node:assert/strict";
import { test } from "node:test";
import {
  initialTranslationState,
  type TranslationResult,
  translationReducer,
} from "../src/popup/translation-state.ts";

const result: TranslationResult = {
  translated: "hello",
  explanation: "greeting",
  source_is_english: false,
};

test("start accepts a newer request_id and resets to loading", () => {
  const state = translationReducer(initialTranslationState, {
    type: "start",
    request_id: 1,
    text: "hi",
  });
  assert.equal(state.requestId, 1);
  assert.equal(state.latestRequestId, 1);
  assert.equal(state.originalText, "hi");
  assert.deepEqual(state.view, { type: "loading" });
});

test("start ignores a request_id at or below latestRequestId", () => {
  const started = translationReducer(initialTranslationState, {
    type: "start",
    request_id: 2,
    text: "hi",
  });
  const stale = translationReducer(started, { type: "start", request_id: 2, text: "again" });
  const older = translationReducer(started, { type: "start", request_id: 1, text: "older" });
  assert.equal(stale, started);
  assert.equal(older, started);
});

test("update is ignored when request_id does not match the active request", () => {
  const started = translationReducer(initialTranslationState, {
    type: "start",
    request_id: 1,
    text: "hi",
  });
  const updated = translationReducer(started, {
    type: "update",
    request_id: 2,
    result,
    complete: true,
  });
  assert.equal(updated, started);
});

test("update applies to the matching request_id", () => {
  const started = translationReducer(initialTranslationState, {
    type: "start",
    request_id: 1,
    text: "hi",
  });
  const updated = translationReducer(started, {
    type: "update",
    request_id: 1,
    result,
    complete: false,
  });
  assert.deepEqual(updated.view, { type: "result", result, complete: false });
});

test("error on a result view marks it complete with an error message instead of replacing it", () => {
  const started = translationReducer(initialTranslationState, {
    type: "start",
    request_id: 1,
    text: "hi",
  });
  const updated = translationReducer(started, {
    type: "update",
    request_id: 1,
    result,
    complete: false,
  });
  const errored = translationReducer(updated, { type: "error", request_id: 1, message: "boom" });
  assert.deepEqual(errored.view, { type: "result", result, complete: true, error: "boom" });
});

test("error on a non-result view replaces it with an error view", () => {
  const started = translationReducer(initialTranslationState, {
    type: "start",
    request_id: 1,
    text: "hi",
  });
  const errored = translationReducer(started, { type: "error", request_id: 1, message: "boom" });
  assert.deepEqual(errored.view, { type: "error", message: "boom" });
});

test("error is ignored when request_id does not match the active request", () => {
  const started = translationReducer(initialTranslationState, {
    type: "start",
    request_id: 1,
    text: "hi",
  });
  const errored = translationReducer(started, { type: "error", request_id: 2, message: "boom" });
  assert.equal(errored, started);
});

test("preflight-error clears requestId and shows an error view regardless of prior state", () => {
  const started = translationReducer(initialTranslationState, {
    type: "start",
    request_id: 1,
    text: "hi",
  });
  const errored = translationReducer(started, { type: "preflight-error", message: "no network" });
  assert.equal(errored.requestId, null);
  assert.deepEqual(errored.view, { type: "error", message: "no network" });
});

test("dismiss clears requestId but leaves the current view untouched", () => {
  const started = translationReducer(initialTranslationState, {
    type: "start",
    request_id: 1,
    text: "hi",
  });
  const updated = translationReducer(started, {
    type: "update",
    request_id: 1,
    result,
    complete: true,
  });
  const dismissed = translationReducer(updated, { type: "dismiss" });
  assert.equal(dismissed.requestId, null);
  assert.deepEqual(dismissed.view, updated.view);
});

test("retry clears requestId and resets the view to loading", () => {
  const started = translationReducer(initialTranslationState, {
    type: "start",
    request_id: 1,
    text: "hi",
  });
  const updated = translationReducer(started, {
    type: "update",
    request_id: 1,
    result,
    complete: true,
  });
  const retried = translationReducer(updated, { type: "retry" });
  assert.equal(retried.requestId, null);
  assert.deepEqual(retried.view, { type: "loading" });
});
