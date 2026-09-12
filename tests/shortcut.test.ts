import assert from "node:assert/strict";
import { test } from "node:test";
import { formatShortcut, type ShortcutKey, shortcutFromKey } from "../src/settings/shortcut.ts";

const key: ShortcutKey = {
  code: "Space",
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
};

test("Option input displays its macOS name while retaining a registerable shortcut", () => {
  const shortcut = shortcutFromKey({ ...key, altKey: true });
  assert.ok(shortcut);
  assert.equal(shortcut, "Alt+Space");
  assert.equal(formatShortcut(shortcut, true), "Option+Space");
  assert.equal(formatShortcut(shortcut, false), "Alt+Space");
});

test("Command input uses Super for native registration and Command for macOS display", () => {
  const shortcut = shortcutFromKey({ ...key, code: "KeyK", metaKey: true, shiftKey: true });
  assert.ok(shortcut);
  assert.equal(shortcut, "Shift+Super+KeyK");
  assert.equal(formatShortcut(shortcut, true), "Shift+Command+K");
  assert.equal(formatShortcut(shortcut, false), "Shift+Meta+K");
});

test("existing shortcut values keep their key formatting and platform labels", () => {
  assert.equal(formatShortcut("Meta+KeyK", true), "Command+K");
  assert.equal(formatShortcut("Cmd+Digit1", true), "Command+1");
  assert.equal(formatShortcut("Ctrl+Alt+Space", false), "Ctrl+Alt+Space");
  assert.equal(formatShortcut("Ctrl+Shift+ArrowLeft", true), "Ctrl+Shift+Left");
  assert.equal(formatShortcut("Alt+Numpad1", true), "Option+Num1");
  assert.equal(formatShortcut("", true), "");
  assert.equal(shortcutFromKey({ ...key, code: "Escape", metaKey: true }), null);
  assert.equal(shortcutFromKey({ ...key, code: "MetaLeft", metaKey: true }), null);
});
