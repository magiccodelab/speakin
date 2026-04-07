const MODIFIER_KEYS = new Set(["Control", "Alt", "Shift", "Meta"]);

const RESERVED_COMBOS = new Set([
  "Ctrl+C", "Ctrl+V", "Ctrl+X", "Ctrl+Z", "Ctrl+A", "Ctrl+S",
  "Ctrl+W", "Ctrl+T", "Ctrl+N", "Ctrl+P", "Ctrl+F", "Ctrl+H",
  "Alt+F4", "Alt+Tab",
]);

const SAFE_SINGLE_KEYS = new Set([
  "F1", "F2", "F3", "F4", "F5", "F6",
  "F7", "F8", "F9", "F10", "F11", "F12",
  "Tab", "Enter", "Escape", "CapsLock", "NumLock", "ScrollLock",
  "PrintScreen", "Pause",
]);

const SPECIAL_KEY_MAP: Record<string, string> = {
  SPACE: "Space",
  TAB: "Tab",
  ENTER: "Enter",
  ESC: "Escape",
  ESCAPE: "Escape",
  BACKSPACE: "Backspace",
  DELETE: "Delete",
  INSERT: "Insert",
  HOME: "Home",
  END: "End",
  PAGEUP: "PageUp",
  PAGEDOWN: "PageDown",
  UP: "Up",
  DOWN: "Down",
  LEFT: "Left",
  RIGHT: "Right",
  CAPS: "CapsLock",
  CAPSLOCK: "CapsLock",
  NUMLOCK: "NumLock",
  SCROLLLOCK: "ScrollLock",
  PRINTSCREEN: "PrintScreen",
  PRTSC: "PrintScreen",
  PAUSE: "Pause",
  NUM0: "Num0",
  NUM1: "Num1",
  NUM2: "Num2",
  NUM3: "Num3",
  NUM4: "Num4",
  NUM5: "Num5",
  NUM6: "Num6",
  NUM7: "Num7",
  NUM8: "Num8",
  NUM9: "Num9",
};

function normalizeTriggerPart(part: string): string | null {
  const trimmed = part.trim();
  const upper = trimmed.toUpperCase();

  if (SPECIAL_KEY_MAP[upper]) {
    return SPECIAL_KEY_MAP[upper];
  }
  if (/^F(?:[1-9]|1[0-2])$/.test(upper)) {
    return upper;
  }
  if (trimmed.length === 1 && /[a-zA-Z]/.test(trimmed)) {
    return trimmed.toUpperCase();
  }
  if (trimmed.length === 1 && /[0-9]/.test(trimmed)) {
    return trimmed;
  }
  if (["`", "-", "=", "[", "]", "\\", ";", "'", ",", ".", "/"].includes(trimmed)) {
    return trimmed;
  }

  return null;
}

export function normalizeHotkeyString(value: string): string | null {
  const parts = value.split("+").map((part) => part.trim()).filter(Boolean);
  if (!parts.length) return null;

  let ctrl = false;
  let alt = false;
  let shift = false;
  let trigger: string | null = null;

  for (const part of parts) {
    const upper = part.toUpperCase();
    if (upper === "CTRL" || upper === "CONTROL") {
      if (ctrl) return null;
      ctrl = true;
      continue;
    }
    if (upper === "ALT") {
      if (alt) return null;
      alt = true;
      continue;
    }
    if (upper === "SHIFT") {
      if (shift) return null;
      shift = true;
      continue;
    }
    if (trigger) return null;

    trigger = normalizeTriggerPart(part);
    if (!trigger) return null;
  }

  if (!trigger) return null;

  const normalizedParts: string[] = [];
  if (ctrl) normalizedParts.push("Ctrl");
  if (alt) normalizedParts.push("Alt");
  if (shift) normalizedParts.push("Shift");
  normalizedParts.push(trigger);
  return normalizedParts.join("+");
}

export function validateHotkeyString(value: string): string | null {
  const normalized = normalizeHotkeyString(value);
  if (!normalized) {
    return "快捷键格式无效，请重新录制";
  }

  if (RESERVED_COMBOS.has(normalized)) {
    return "该快捷键与系统或常用编辑操作冲突，请更换";
  }

  const parts = normalized.split("+");
  const trigger = parts[parts.length - 1];
  if (parts.length === 1 && !SAFE_SINGLE_KEYS.has(trigger)) {
    return "该按键单独使用会影响正常输入，请至少添加 Ctrl、Alt 或 Shift";
  }

  return null;
}

/** Map KeyboardEvent.key to the display name used in hotkey strings */
export function keyToHotkeyName(e: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null;
  if (/^F(?:[1-9]|1[0-2])$/.test(e.key)) return e.key;
  if (e.key.length === 1 && /[a-zA-Z0-9]/.test(e.key)) return e.key.toUpperCase();

  const map: Record<string, string> = {
    " ": "Space",
    Tab: "Tab",
    Enter: "Enter",
    Escape: "Escape",
    Backspace: "Backspace",
    Delete: "Delete",
    Insert: "Insert",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
    CapsLock: "CapsLock",
    NumLock: "NumLock",
    ScrollLock: "ScrollLock",
    PrintScreen: "PrintScreen",
    Pause: "Pause",
    "`": "`",
    "-": "-",
    "=": "=",
    "[": "[",
    "]": "]",
    "\\": "\\",
    ";": ";",
    "'": "'",
    ",": ",",
    ".": ".",
    "/": "/",
  };
  return map[e.key] ?? null;
}

/** Build a hotkey string from a KeyboardEvent */
export function buildHotkeyString(e: KeyboardEvent): string | null {
  const keyName = keyToHotkeyName(e);
  if (!keyName) return null;

  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(keyName);

  return parts.join("+");
}
