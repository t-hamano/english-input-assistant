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
  // The native parser calls the Command/Windows modifier "Super", not "Meta".
  if (key.metaKey) parts.push("Super");
  if (parts.length === 0) return null;
  parts.push(key.code);
  return parts.join("+");
}

function formatKeyName(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (code.startsWith("Numpad")) return `Num${code.slice(6)}`;
  if (code.startsWith("Arrow")) return code.slice(5);
  return code;
}

export function formatShortcut(shortcut: string, isMac: boolean): string {
  if (!shortcut) return "";
  const parts = shortcut.split("+");
  // biome-ignore lint/style/noNonNullAssertion: split() on a non-empty string always yields at least one part
  const key = parts.pop()!;
  // Keep the stored modifier names compatible with the native shortcut parser.
  const modifiers = parts.map((part) => {
    if (isMac && part === "Alt") return "Option";
    if (isMac && (part === "Meta" || part === "Super" || part === "Cmd")) return "Command";
    if (!isMac && part === "Super") return "Meta";
    return part;
  });
  return [...modifiers, formatKeyName(key)].join("+");
}
