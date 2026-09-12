export interface ShortcutKey {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
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

export function shortcutFromKey(key: ShortcutKey): string | null {
  if (key.code === "Escape" || MODIFIER_CODES.has(key.code)) return null;
  const parts: string[] = [];
  if (key.ctrlKey) parts.push("Ctrl");
  if (key.altKey) parts.push("Alt");
  if (key.shiftKey) parts.push("Shift");
  if (key.metaKey) parts.push("Meta");
  if (parts.length === 0) return null;
  parts.push(key.code);
  return parts.join("+");
}
