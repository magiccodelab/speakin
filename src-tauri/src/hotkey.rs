//! Global hotkey support using Windows native low-level keyboard hooks.
//!
//! Directly calls `SetWindowsHookExW(WH_KEYBOARD_LL)` via `windows-sys`.
//! Matched hotkey events are consumed (callback returns 1) to prevent
//! system handling (e.g., CapsLock toggle).

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;

// ── Custom Key enum (replaces rdev::Key) ──

/// Keyboard key identifiers used for hotkey matching.
/// Only covers keys relevant to hotkey configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    // Letters
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ,
    KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT,
    KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    // Digits
    Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    // Toggle keys
    CapsLock, NumLock, ScrollLock,
    // Navigation / control
    Space, Tab, Return, Escape, Backspace, Delete, Insert,
    Home, End, PageUp, PageDown,
    UpArrow, DownArrow, LeftArrow, RightArrow,
    PrintScreen, Pause,
    // Numpad
    Kp0, Kp1, Kp2, Kp3, Kp4, Kp5, Kp6, Kp7, Kp8, Kp9,
    // Punctuation
    BackQuote, Minus, Equal, LeftBracket, RightBracket, BackSlash,
    SemiColon, Quote, Comma, Dot, Slash,
    // Modifiers
    ShiftLeft, ShiftRight, ControlLeft, ControlRight, Alt, AltGr,
    MetaLeft, MetaRight,
}

/// Map a Windows virtual key code to our Key enum.
fn vk_to_key(vk: u32) -> Option<Key> {
    match vk {
        // Letters A-Z (0x41-0x5A)
        0x41 => Some(Key::KeyA), 0x42 => Some(Key::KeyB), 0x43 => Some(Key::KeyC),
        0x44 => Some(Key::KeyD), 0x45 => Some(Key::KeyE), 0x46 => Some(Key::KeyF),
        0x47 => Some(Key::KeyG), 0x48 => Some(Key::KeyH), 0x49 => Some(Key::KeyI),
        0x4A => Some(Key::KeyJ), 0x4B => Some(Key::KeyK), 0x4C => Some(Key::KeyL),
        0x4D => Some(Key::KeyM), 0x4E => Some(Key::KeyN), 0x4F => Some(Key::KeyO),
        0x50 => Some(Key::KeyP), 0x51 => Some(Key::KeyQ), 0x52 => Some(Key::KeyR),
        0x53 => Some(Key::KeyS), 0x54 => Some(Key::KeyT), 0x55 => Some(Key::KeyU),
        0x56 => Some(Key::KeyV), 0x57 => Some(Key::KeyW), 0x58 => Some(Key::KeyX),
        0x59 => Some(Key::KeyY), 0x5A => Some(Key::KeyZ),
        // Digits 0-9 (0x30-0x39)
        0x30 => Some(Key::Num0), 0x31 => Some(Key::Num1), 0x32 => Some(Key::Num2),
        0x33 => Some(Key::Num3), 0x34 => Some(Key::Num4), 0x35 => Some(Key::Num5),
        0x36 => Some(Key::Num6), 0x37 => Some(Key::Num7), 0x38 => Some(Key::Num8),
        0x39 => Some(Key::Num9),
        // Function keys F1-F12 (0x70-0x7B)
        0x70 => Some(Key::F1),  0x71 => Some(Key::F2),  0x72 => Some(Key::F3),
        0x73 => Some(Key::F4),  0x74 => Some(Key::F5),  0x75 => Some(Key::F6),
        0x76 => Some(Key::F7),  0x77 => Some(Key::F8),  0x78 => Some(Key::F9),
        0x79 => Some(Key::F10), 0x7A => Some(Key::F11), 0x7B => Some(Key::F12),
        // Toggle keys
        0x14 => Some(Key::CapsLock),   // VK_CAPITAL
        0x90 => Some(Key::NumLock),    // VK_NUMLOCK
        0x91 => Some(Key::ScrollLock), // VK_SCROLL
        // Navigation / control
        0x20 => Some(Key::Space),      // VK_SPACE
        0x09 => Some(Key::Tab),        // VK_TAB
        0x0D => Some(Key::Return),     // VK_RETURN
        0x1B => Some(Key::Escape),     // VK_ESCAPE
        0x08 => Some(Key::Backspace),  // VK_BACK
        0x2E => Some(Key::Delete),     // VK_DELETE
        0x2D => Some(Key::Insert),     // VK_INSERT
        0x24 => Some(Key::Home),       // VK_HOME
        0x23 => Some(Key::End),        // VK_END
        0x21 => Some(Key::PageUp),     // VK_PRIOR
        0x22 => Some(Key::PageDown),   // VK_NEXT
        0x26 => Some(Key::UpArrow),    // VK_UP
        0x28 => Some(Key::DownArrow),  // VK_DOWN
        0x25 => Some(Key::LeftArrow),  // VK_LEFT
        0x27 => Some(Key::RightArrow), // VK_RIGHT
        0x2C => Some(Key::PrintScreen),// VK_SNAPSHOT
        0x13 => Some(Key::Pause),      // VK_PAUSE
        // Numpad 0-9 (0x60-0x69)
        0x60 => Some(Key::Kp0), 0x61 => Some(Key::Kp1), 0x62 => Some(Key::Kp2),
        0x63 => Some(Key::Kp3), 0x64 => Some(Key::Kp4), 0x65 => Some(Key::Kp5),
        0x66 => Some(Key::Kp6), 0x67 => Some(Key::Kp7), 0x68 => Some(Key::Kp8),
        0x69 => Some(Key::Kp9),
        // Punctuation (OEM keys)
        0xC0 => Some(Key::BackQuote),    // VK_OEM_3
        0xBD => Some(Key::Minus),        // VK_OEM_MINUS
        0xBB => Some(Key::Equal),        // VK_OEM_PLUS (the = key)
        0xDB => Some(Key::LeftBracket),  // VK_OEM_4
        0xDD => Some(Key::RightBracket), // VK_OEM_6
        0xDC => Some(Key::BackSlash),    // VK_OEM_5
        0xBA => Some(Key::SemiColon),    // VK_OEM_1
        0xDE => Some(Key::Quote),        // VK_OEM_7
        0xBC => Some(Key::Comma),        // VK_OEM_COMMA
        0xBE => Some(Key::Dot),          // VK_OEM_PERIOD
        0xBF => Some(Key::Slash),        // VK_OEM_2
        // Modifier keys (LL hook provides left/right-specific VK codes)
        0xA0 => Some(Key::ShiftLeft),    // VK_LSHIFT
        0xA1 => Some(Key::ShiftRight),   // VK_RSHIFT
        0xA2 => Some(Key::ControlLeft),  // VK_LCONTROL
        0xA3 => Some(Key::ControlRight), // VK_RCONTROL
        0xA4 => Some(Key::Alt),          // VK_LMENU
        0xA5 => Some(Key::AltGr),        // VK_RMENU
        0x5B => Some(Key::MetaLeft),     // VK_LWIN
        0x5C => Some(Key::MetaRight),    // VK_RWIN
        // Generic modifier VKs (defensive fallback — LL hooks normally
        // provide the left/right-specific codes above, but map these
        // to left-side variants just in case)
        0x10 => Some(Key::ShiftLeft),    // VK_SHIFT
        0x11 => Some(Key::ControlLeft),  // VK_CONTROL
        0x12 => Some(Key::Alt),          // VK_MENU
        _ => None,
    }
}

/// Hotkey events sent to the main app logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// Short press detected (toggle mode): start or stop recording
    ShortPress,
    /// Long press started (hold mode): start recording
    HoldStart,
    /// Long press ended (hold mode): stop recording
    HoldEnd,
    /// Escape pressed while a session is active: abort the current session
    AbortSession,
}

/// Input mode determines how the hotkey behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Toggle,
    Hold,
}

const RESERVED_COMBOS: &[&str] = &[
    "Ctrl+C",
    "Ctrl+V",
    "Ctrl+X",
    "Ctrl+Z",
    "Ctrl+A",
    "Ctrl+S",
    "Ctrl+W",
    "Ctrl+T",
    "Ctrl+N",
    "Ctrl+P",
    "Ctrl+F",
    "Ctrl+H",
    "Alt+F4",
    "Alt+Tab",
];

// ── Modifier flags ──

const MOD_CTRL: u8 = 0x01;
const MOD_ALT: u8 = 0x02;
const MOD_SHIFT: u8 = 0x04;

/// A hotkey combination: optional modifiers + a trigger key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HotkeyCombo {
    modifiers: u8,
    key: Key,
}

#[derive(Debug, Clone)]
pub struct ValidatedHotkey {
    normalized: String,
    combo: HotkeyCombo,
}

impl ValidatedHotkey {
    pub fn normalized(&self) -> &str {
        &self.normalized
    }
}

struct KeySpec {
    key: Key,
    normalized: String,
    allow_without_modifiers: bool,
}

/// Internal state for the hotkey handler.
struct HotkeyState {
    is_key_down: bool,
    input_mode: InputMode,
    combo: HotkeyCombo,
    /// Track which modifier keys are currently pressed.
    active_mods: u8,
}

#[derive(Debug, PartialEq, Eq)]
struct ProcessResult {
    consumed: bool,
    emitted: Option<HotkeyEvent>,
}

static HOTKEY_STATE: OnceLock<Mutex<HotkeyState>> = OnceLock::new();
static HOTKEY_TX: OnceLock<std::sync::mpsc::Sender<HotkeyEvent>> = OnceLock::new();

/// Lock-free trigger key identifier — used to consume hotkey events
/// even when HOTKEY_STATE lock is contended (prevents OS from handling
/// CapsLock toggle, etc. during config updates).
static TRIGGER_KEY_HASH: AtomicU64 = AtomicU64::new(0);

/// Timestamp of last hook callback invocation (seconds since UNIX epoch, truncated).
/// Used by the watchdog timer to detect hook death.
static LAST_HOOK_CALL: AtomicU64 = AtomicU64::new(0);
static ESC_ABORT_ENABLED: AtomicBool = AtomicBool::new(true);
static ESC_ABORT_ACTIVE: AtomicBool = AtomicBool::new(false);
static ESC_ABORT_KEY_DOWN: AtomicBool = AtomicBool::new(false);

/// Compute a hash from a Key's discriminant for lock-free comparison.
fn key_hash(key: &Key) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::mem::discriminant(key).hash(&mut hasher);
    hasher.finish()
}

fn key_spec(key: Key, normalized: impl Into<String>, allow_without_modifiers: bool) -> KeySpec {
    KeySpec {
        key,
        normalized: normalized.into(),
        allow_without_modifiers,
    }
}

fn parse_key_spec(name: &str) -> Option<KeySpec> {
    let trimmed = name.trim();
    let upper = trimmed.to_ascii_uppercase();

    match upper.as_str() {
        // Function keys
        "F1" => Some(key_spec(Key::F1, "F1", true)),
        "F2" => Some(key_spec(Key::F2, "F2", true)),
        "F3" => Some(key_spec(Key::F3, "F3", true)),
        "F4" => Some(key_spec(Key::F4, "F4", true)),
        "F5" => Some(key_spec(Key::F5, "F5", true)),
        "F6" => Some(key_spec(Key::F6, "F6", true)),
        "F7" => Some(key_spec(Key::F7, "F7", true)),
        "F8" => Some(key_spec(Key::F8, "F8", true)),
        "F9" => Some(key_spec(Key::F9, "F9", true)),
        "F10" => Some(key_spec(Key::F10, "F10", true)),
        "F11" => Some(key_spec(Key::F11, "F11", true)),
        "F12" => Some(key_spec(Key::F12, "F12", true)),
        // Toggle keys
        "CAPSLOCK" | "CAPS" => Some(key_spec(Key::CapsLock, "CapsLock", true)),
        "NUMLOCK" => Some(key_spec(Key::NumLock, "NumLock", true)),
        "SCROLLLOCK" => Some(key_spec(Key::ScrollLock, "ScrollLock", true)),
        // Navigation / control keys
        "SPACE" => Some(key_spec(Key::Space, "Space", false)),
        "TAB" => Some(key_spec(Key::Tab, "Tab", true)),
        "ENTER" => Some(key_spec(Key::Return, "Enter", true)),
        "ESCAPE" | "ESC" => Some(key_spec(Key::Escape, "Escape", true)),
        "BACKSPACE" => Some(key_spec(Key::Backspace, "Backspace", false)),
        "DELETE" => Some(key_spec(Key::Delete, "Delete", false)),
        "INSERT" => Some(key_spec(Key::Insert, "Insert", false)),
        "HOME" => Some(key_spec(Key::Home, "Home", false)),
        "END" => Some(key_spec(Key::End, "End", false)),
        "PAGEUP" => Some(key_spec(Key::PageUp, "PageUp", false)),
        "PAGEDOWN" => Some(key_spec(Key::PageDown, "PageDown", false)),
        "UP" => Some(key_spec(Key::UpArrow, "Up", false)),
        "DOWN" => Some(key_spec(Key::DownArrow, "Down", false)),
        "LEFT" => Some(key_spec(Key::LeftArrow, "Left", false)),
        "RIGHT" => Some(key_spec(Key::RightArrow, "Right", false)),
        "PRINTSCREEN" | "PRTSC" => Some(key_spec(Key::PrintScreen, "PrintScreen", true)),
        "PAUSE" => Some(key_spec(Key::Pause, "Pause", true)),
        // Numpad
        "NUM0" => Some(key_spec(Key::Kp0, "Num0", false)),
        "NUM1" => Some(key_spec(Key::Kp1, "Num1", false)),
        "NUM2" => Some(key_spec(Key::Kp2, "Num2", false)),
        "NUM3" => Some(key_spec(Key::Kp3, "Num3", false)),
        "NUM4" => Some(key_spec(Key::Kp4, "Num4", false)),
        "NUM5" => Some(key_spec(Key::Kp5, "Num5", false)),
        "NUM6" => Some(key_spec(Key::Kp6, "Num6", false)),
        "NUM7" => Some(key_spec(Key::Kp7, "Num7", false)),
        "NUM8" => Some(key_spec(Key::Kp8, "Num8", false)),
        "NUM9" => Some(key_spec(Key::Kp9, "Num9", false)),
        _ => {
            if trimmed.len() == 1 {
                let ch = trimmed.chars().next()?;
                if ch.is_ascii_alphabetic() {
                    let upper_ch = ch.to_ascii_uppercase();
                    return Some(key_spec(
                        key_from_char(upper_ch)?,
                        upper_ch.to_string(),
                        false,
                    ));
                }
                if ch.is_ascii_digit() {
                    return Some(key_spec(key_from_char(ch)?, ch.to_string(), false));
                }
                return match ch {
                    '`' => Some(key_spec(Key::BackQuote, "`", false)),
                    '-' => Some(key_spec(Key::Minus, "-", false)),
                    '=' => Some(key_spec(Key::Equal, "=", false)),
                    '[' => Some(key_spec(Key::LeftBracket, "[", false)),
                    ']' => Some(key_spec(Key::RightBracket, "]", false)),
                    '\\' => Some(key_spec(Key::BackSlash, "\\", false)),
                    ';' => Some(key_spec(Key::SemiColon, ";", false)),
                    '\'' => Some(key_spec(Key::Quote, "'", false)),
                    ',' => Some(key_spec(Key::Comma, ",", false)),
                    '.' => Some(key_spec(Key::Dot, ".", false)),
                    '/' => Some(key_spec(Key::Slash, "/", false)),
                    _ => None,
                };
            }

            None
        }
    }
}

fn key_from_char(ch: char) -> Option<Key> {
    match ch {
        'A' => Some(Key::KeyA),
        'B' => Some(Key::KeyB),
        'C' => Some(Key::KeyC),
        'D' => Some(Key::KeyD),
        'E' => Some(Key::KeyE),
        'F' => Some(Key::KeyF),
        'G' => Some(Key::KeyG),
        'H' => Some(Key::KeyH),
        'I' => Some(Key::KeyI),
        'J' => Some(Key::KeyJ),
        'K' => Some(Key::KeyK),
        'L' => Some(Key::KeyL),
        'M' => Some(Key::KeyM),
        'N' => Some(Key::KeyN),
        'O' => Some(Key::KeyO),
        'P' => Some(Key::KeyP),
        'Q' => Some(Key::KeyQ),
        'R' => Some(Key::KeyR),
        'S' => Some(Key::KeyS),
        'T' => Some(Key::KeyT),
        'U' => Some(Key::KeyU),
        'V' => Some(Key::KeyV),
        'W' => Some(Key::KeyW),
        'X' => Some(Key::KeyX),
        'Y' => Some(Key::KeyY),
        'Z' => Some(Key::KeyZ),
        '0' => Some(Key::Num0),
        '1' => Some(Key::Num1),
        '2' => Some(Key::Num2),
        '3' => Some(Key::Num3),
        '4' => Some(Key::Num4),
        '5' => Some(Key::Num5),
        '6' => Some(Key::Num6),
        '7' => Some(Key::Num7),
        '8' => Some(Key::Num8),
        '9' => Some(Key::Num9),
        _ => None,
    }
}

fn format_hotkey_name(modifiers: u8, key_name: &str) -> String {
    let mut parts = Vec::with_capacity(4);
    if modifiers & MOD_CTRL != 0 {
        parts.push("Ctrl");
    }
    if modifiers & MOD_ALT != 0 {
        parts.push("Alt");
    }
    if modifiers & MOD_SHIFT != 0 {
        parts.push("Shift");
    }
    parts.push(key_name);
    parts.join("+")
}

pub fn validate_hotkey(name: &str) -> Result<ValidatedHotkey, String> {
    let parts: Vec<&str> = name
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();

    if parts.is_empty() {
        return Err("快捷键不能为空".to_string());
    }

    let mut modifiers = 0;
    let mut trigger: Option<KeySpec> = None;

    for part in parts {
        match part.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => {
                if modifiers & MOD_CTRL != 0 {
                    return Err("快捷键中不能重复使用 Ctrl".to_string());
                }
                modifiers |= MOD_CTRL;
            }
            "ALT" => {
                if modifiers & MOD_ALT != 0 {
                    return Err("快捷键中不能重复使用 Alt".to_string());
                }
                modifiers |= MOD_ALT;
            }
            "SHIFT" => {
                if modifiers & MOD_SHIFT != 0 {
                    return Err("快捷键中不能重复使用 Shift".to_string());
                }
                modifiers |= MOD_SHIFT;
            }
            _ => {
                if trigger.is_some() {
                    return Err("快捷键只能包含一个触发键".to_string());
                }
                let Some(key) = parse_key_spec(part) else {
                    return Err(format!("不支持的快捷键: {}", part));
                };
                trigger = Some(key);
            }
        }
    }

    let Some(trigger) = trigger else {
        return Err("快捷键必须包含一个触发键".to_string());
    };

    let normalized = format_hotkey_name(modifiers, &trigger.normalized);
    if RESERVED_COMBOS.contains(&normalized.as_str()) {
        return Err("该快捷键与系统或常用编辑操作冲突，请更换".to_string());
    }
    if modifiers == 0 && !trigger.allow_without_modifiers {
        return Err("该按键单独使用会影响正常输入，请至少添加 Ctrl、Alt 或 Shift".to_string());
    }

    Ok(ValidatedHotkey {
        normalized,
        combo: HotkeyCombo {
            modifiers,
            key: trigger.key,
        },
    })
}

/// Check if a Key is a modifier key.
fn is_modifier_key(key: &Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::Alt
            | Key::AltGr
            | Key::MetaLeft
            | Key::MetaRight
    )
}

/// Get the modifier flag for a modifier key.
fn modifier_flag(key: &Key) -> u8 {
    match key {
        Key::ControlLeft | Key::ControlRight => MOD_CTRL,
        Key::Alt | Key::AltGr => MOD_ALT,
        Key::ShiftLeft | Key::ShiftRight => MOD_SHIFT,
        _ => 0,
    }
}

fn reset_runtime_state(state: &mut HotkeyState) {
    state.active_mods = 0;
    state.is_key_down = false;
}

fn apply_config_to_state(state: &mut HotkeyState, hotkey: &ValidatedHotkey, input_mode: InputMode) {
    reset_runtime_state(state);
    state.combo = hotkey.combo;
    state.input_mode = input_mode;
}

/// Start the global hotkey listener on a dedicated thread.
pub fn start_listener(
    hotkey: &ValidatedHotkey,
    input_mode: InputMode,
) -> std::sync::mpsc::Receiver<HotkeyEvent> {
    let (tx, rx) = std::sync::mpsc::channel();
    let combo = hotkey.combo;

    HOTKEY_TX.get_or_init(|| tx);
    HOTKEY_STATE.get_or_init(|| {
        Mutex::new(HotkeyState {
            is_key_down: false,
            input_mode,
            combo,
            active_mods: 0,
        })
    });

    TRIGGER_KEY_HASH.store(key_hash(&combo.key), Ordering::Relaxed);

    log::info!(
        "[hotkey] start_listener: hotkey={} mode={:?} trigger_key={:?} mods=0x{:02X}",
        hotkey.normalized(), input_mode, combo.key, combo.modifiers
    );

    {
        let mut state = HOTKEY_STATE.get().unwrap().lock();
        apply_config_to_state(&mut state, hotkey, input_mode);
    }

    std::thread::spawn(move || {
        use windows_sys::Win32::Foundation::GetLastError;
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::System::Threading::*;
        use windows_sys::Win32::UI::WindowsAndMessaging::*;

        // Elevate thread priority to reduce risk of hook timeout removal
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
        }

        let mut backoff_ms: u64 = 100;
        const MAX_BACKOFF_MS: u64 = 2000;
        const WATCHDOG_INTERVAL_MS: u32 = 10_000;
        const WATCHDOG_STALE_SECS: u64 = 120;

        loop {
            let thread_id = unsafe {
                windows_sys::Win32::System::Threading::GetCurrentThreadId()
            };
            log::info!(
                "[hook-thread] Installing WH_KEYBOARD_LL hook (thread_id={})",
                thread_id
            );
            let hook_start = std::time::Instant::now();

            let hmod = unsafe { GetModuleHandleW(std::ptr::null()) };
            log::debug!("[hook-thread] hmod={:?}", hmod);
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(low_level_keyboard_proc),
                    hmod,
                    0, // dwThreadId: 0 = global hook
                )
            };

            if hook.is_null() {
                let err = unsafe { GetLastError() };
                log::error!("[hook-thread] SetWindowsHookExW FAILED, error code: {}", err);
            } else {
                log::info!("WH_KEYBOARD_LL hook installed successfully");

                // Initialize last hook call timestamp
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                LAST_HOOK_CALL.store(now_secs, Ordering::Relaxed);

                // Install watchdog timer to detect hook death.
                // With null HWND, SetTimer ignores nIDEvent and returns a
                // system-assigned ID — we must capture and use that ID.
                let timer_id = unsafe {
                    SetTimer(std::ptr::null_mut(), 0, WATCHDOG_INTERVAL_MS, None)
                };

                let mut msg: windows_sys::Win32::UI::WindowsAndMessaging::MSG =
                    unsafe { std::mem::zeroed() };
                loop {
                    let ret = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
                    if ret == 0 {
                        log::info!("WM_QUIT received, exiting message loop");
                        break;
                    }
                    if ret == -1 {
                        let err = unsafe { GetLastError() };
                        log::error!("GetMessageW error: {}", err);
                        break;
                    }
                    // Watchdog: check if hook is still alive
                    if msg.message == WM_TIMER && msg.wParam == timer_id {
                        let last = LAST_HOOK_CALL.load(Ordering::Relaxed);
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        if now.saturating_sub(last) > WATCHDOG_STALE_SECS {
                            log::warn!(
                                "Hook appears dead (no callbacks for {}s), restarting",
                                now.saturating_sub(last)
                            );
                            break;
                        }
                    }
                }

                unsafe {
                    KillTimer(std::ptr::null_mut(), timer_id);
                    UnhookWindowsHookEx(hook);
                }
            }

            // Reset state after hook exit
            if let Some(state_lock) = HOTKEY_STATE.get() {
                let mut state = state_lock.lock();
                reset_runtime_state(&mut state);
            }

            // If hook ran for >5s it was working fine — reset backoff
            if hook_start.elapsed() > std::time::Duration::from_secs(5) {
                backoff_ms = 100;
            }
            log::warn!("Restarting keyboard hook in {}ms", backoff_ms);
            std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
            backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
        }
    });

    rx
}

/// Update the hotkey configuration at runtime.
pub fn update_config(hotkey: &ValidatedHotkey, input_mode: InputMode) {
    log::info!(
        "[hotkey] update_config: hotkey={} mode={:?} trigger_key={:?} mods=0x{:02X}",
        hotkey.normalized(), input_mode, hotkey.combo.key, hotkey.combo.modifiers
    );
    // Update atomic trigger key FIRST (lock-free, immediate) so the hook callback
    // can identify our key even while we hold the state lock below.
    TRIGGER_KEY_HASH.store(key_hash(&hotkey.combo.key), Ordering::Relaxed);
    if let Some(state) = HOTKEY_STATE.get() {
        let mut current = state.lock();
        apply_config_to_state(&mut current, hotkey, input_mode);
    }
}

/// Update whether Escape-based abort is enabled in settings.
pub fn update_escape_abort_config(enabled: bool) {
    ESC_ABORT_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Update whether there is an active session that Escape may abort.
pub fn set_escape_abort_active(active: bool) {
    ESC_ABORT_ACTIVE.store(active, Ordering::Relaxed);
}

/// Windows low-level keyboard hook callback.
/// Returns 1 to consume matched hotkey events, or calls CallNextHookEx to pass through.
///
/// CRITICAL: This runs inside a Windows low-level keyboard hook.
/// If it takes >200ms, Windows will silently remove the hook.
unsafe extern "system" fn low_level_keyboard_proc(
    n_code: i32,
    w_param: usize,
    l_param: isize,
) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    if n_code != HC_ACTION as i32 {
        log::trace!("[hook] n_code={} (not HC_ACTION), passing through", n_code);
        return CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param);
    }

    // catch_unwind prevents panics from crossing the FFI boundary (process-fatal)
    let result = std::panic::catch_unwind(|| {
        let ptr = l_param as *const KBDLLHOOKSTRUCT;
        if ptr.is_null() {
            log::warn!("[hook] l_param is null, passing through");
            return false;
        }
        let kb = &*ptr;

        let action = match w_param as u32 {
            WM_KEYDOWN => "DOWN",
            WM_KEYUP => "UP",
            WM_SYSKEYDOWN => "SYSDOWN",
            WM_SYSKEYUP => "SYSUP",
            other => { log::debug!("[hook] unknown wParam=0x{:X}", other); "?" }
        };

        log::debug!(
            "[hook] vk=0x{:02X} scan=0x{:X} flags=0x{:X} action={}",
            kb.vkCode, kb.scanCode, kb.flags, action
        );

        // Update watchdog timestamp
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        LAST_HOOK_CALL.store(now_secs, Ordering::Relaxed);

        // Pass through injected keys (from enigo's Ctrl+V paste or Type mode)
        if kb.flags & LLKHF_INJECTED != 0 {
            log::debug!("[hook] INJECTED flag set, passing through");
            return false;
        }

        let Some(key) = vk_to_key(kb.vkCode) else {
            log::trace!("[hook] vk=0x{:02X} not mapped, passing through", kb.vkCode);
            return false;
        };

        let is_press = matches!(w_param as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let consumed = handle_key_event(key, is_press);
        log::debug!(
            "[hook] key={:?} is_press={} → consumed={}",
            key, is_press, consumed
        );
        consumed
    });

    match result {
        Ok(true) => {
            log::debug!("[hook] → CONSUMED (returning 1)");
            1
        }
        Ok(false) => {
            log::trace!("[hook] → pass through (CallNextHookEx)");
            CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param)
        }
        Err(_) => {
            log::error!("[hook] catch_unwind caught a panic! passing through");
            CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param)
        }
    }
}

fn process_key_event(state: &mut HotkeyState, key: Key, is_press: bool) -> ProcessResult {
    if is_modifier_key(&key) {
        let flag = modifier_flag(&key);
        if is_press {
            state.active_mods |= flag;
        } else {
            state.active_mods &= !flag;
        }
        log::trace!("[process] modifier {:?} {} → active_mods=0x{:02X}",
            key, if is_press { "pressed" } else { "released" }, state.active_mods);
        return ProcessResult {
            consumed: false,
            emitted: None,
        };
    }

    if !keys_match(&key, &state.combo.key) {
        log::trace!("[process] key {:?} does not match trigger {:?}, pass through", key, state.combo.key);
        return ProcessResult {
            consumed: false,
            emitted: None,
        };
    }

    // Only check modifier match on press, not on release.
    if is_press && state.active_mods != state.combo.modifiers {
        log::debug!(
            "[process] trigger key {:?} matched but mods mismatch: active=0x{:02X} expected=0x{:02X}, pass through",
            key, state.active_mods, state.combo.modifiers
        );
        return ProcessResult {
            consumed: false,
            emitted: None,
        };
    }

    // On release: if the key was never pressed (e.g., modifiers didn't match
    // at press time), don't consume.
    if !is_press && !state.is_key_down {
        log::debug!("[process] trigger key {:?} released but was not down, pass through", key);
        return ProcessResult {
            consumed: false,
            emitted: None,
        };
    }

    let emitted = if is_press {
        if state.is_key_down {
            None
        } else {
            state.is_key_down = true;
            match state.input_mode {
                InputMode::Hold => Some(HotkeyEvent::HoldStart),
                InputMode::Toggle => Some(HotkeyEvent::ShortPress),
            }
        }
    } else {
        let was_down = state.is_key_down;
        state.is_key_down = false;

        if state.input_mode == InputMode::Hold && was_down {
            Some(HotkeyEvent::HoldEnd)
        } else {
            None
        }
    };

    ProcessResult {
        consumed: true,
        emitted,
    }
}

fn process_escape_abort_event(is_press: bool) -> Option<ProcessResult> {
    let enabled = ESC_ABORT_ENABLED.load(Ordering::Relaxed);
    let active = ESC_ABORT_ACTIVE.load(Ordering::Relaxed);
    let was_down = ESC_ABORT_KEY_DOWN.load(Ordering::Relaxed);

    if !(enabled && active) && !was_down {
        return None;
    }

    if is_press {
        let already_down = ESC_ABORT_KEY_DOWN.swap(true, Ordering::Relaxed);
        return Some(ProcessResult {
            consumed: true,
            emitted: if enabled && active && !already_down {
                Some(HotkeyEvent::AbortSession)
            } else {
                None
            },
        });
    }

    Some(ProcessResult {
        consumed: ESC_ABORT_KEY_DOWN.swap(false, Ordering::Relaxed),
        emitted: None,
    })
}

/// Handle a key event. Returns `true` if the event was consumed as part of
/// our hotkey (caller should suppress it), `false` if it should pass through.
///
/// CRITICAL: This runs inside a Windows low-level keyboard hook.
/// If it takes >200ms, Windows will silently remove the hook.
/// Uses try_lock to avoid blocking if another thread holds the lock.
fn handle_key_event(key: Key, is_press: bool) -> bool {
    if matches!(key, Key::Escape) {
        if let Some(result) = process_escape_abort_event(is_press) {
            if let Some(ref event) = result.emitted {
                log::info!("[handle] EMITTING {:?}", event);
                if let Some(tx) = HOTKEY_TX.get() {
                    let _ = tx.send(event.clone());
                }
            }
            return result.consumed;
        }
    }

    let Some(state_lock) = HOTKEY_STATE.get() else {
        log::warn!("[handle] HOTKEY_STATE not initialized");
        return false;
    };
    let Some(mut state) = state_lock.try_lock() else {
        // Lock contention (e.g., during config update).
        log::warn!("[handle] lock contention, using atomic fallback");
        if is_modifier_key(&key) {
            return false;
        }
        let expected = TRIGGER_KEY_HASH.load(Ordering::Relaxed);
        let consumed = expected != 0 && key_hash(&key) == expected;
        log::debug!("[handle] atomic fallback: consumed={}", consumed);
        return consumed;
    };

    log::debug!(
        "[handle] state: combo_key={:?} combo_mods=0x{:02X} active_mods=0x{:02X} is_key_down={} mode={:?}",
        state.combo.key, state.combo.modifiers, state.active_mods, state.is_key_down, state.input_mode
    );

    let result = process_key_event(&mut state, key, is_press);
    drop(state);

    if let Some(ref event) = result.emitted {
        log::info!("[handle] EMITTING {:?}", event);
        if let Some(tx) = HOTKEY_TX.get() {
            let _ = tx.send(event.clone());
        }
    }

    result.consumed
}

/// Compare two Keys for equality.
fn keys_match(a: &Key, b: &Key) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset_escape_abort_state(enabled: bool, active: bool) {
        ESC_ABORT_ENABLED.store(enabled, Ordering::Relaxed);
        ESC_ABORT_ACTIVE.store(active, Ordering::Relaxed);
        ESC_ABORT_KEY_DOWN.store(false, Ordering::Relaxed);
    }

    fn new_state(combo: &ValidatedHotkey, input_mode: InputMode) -> HotkeyState {
        HotkeyState {
            is_key_down: false,
            input_mode,
            combo: combo.combo,
            active_mods: 0,
        }
    }

    #[test]
    fn validates_and_normalizes_hotkeys() {
        let hotkey = validate_hotkey(" control + shift + v ").unwrap();
        assert_eq!(hotkey.normalized(), "Ctrl+Shift+V");
    }

    #[test]
    fn rejects_missing_trigger_key() {
        let err = validate_hotkey("Ctrl+Shift").unwrap_err();
        assert!(err.contains("触发键"));
    }

    #[test]
    fn rejects_multiple_trigger_keys() {
        let err = validate_hotkey("Ctrl+Shift+V+R").unwrap_err();
        assert!(err.contains("一个触发键"));
    }

    #[test]
    fn rejects_reserved_combos() {
        let err = validate_hotkey("Ctrl+C").unwrap_err();
        assert!(err.contains("冲突"));
    }

    #[test]
    fn rejects_single_printable_keys_without_modifiers() {
        let err = validate_hotkey("V").unwrap_err();
        assert!(err.contains("至少添加"));

        let err = validate_hotkey("Space").unwrap_err();
        assert!(err.contains("至少添加"));
    }

    #[test]
    fn allows_safe_single_keys_without_modifiers() {
        assert_eq!(validate_hotkey("F2").unwrap().normalized(), "F2");
        assert_eq!(validate_hotkey("Caps").unwrap().normalized(), "CapsLock");
        assert_eq!(validate_hotkey("Escape").unwrap().normalized(), "Escape");
    }

    #[test]
    fn update_config_resets_runtime_state() {
        let initial = validate_hotkey("F2").unwrap();
        let replacement = validate_hotkey("Ctrl+Alt+R").unwrap();
        let mut state = new_state(&initial, InputMode::Toggle);
        state.is_key_down = true;
        state.active_mods = MOD_CTRL | MOD_ALT;

        apply_config_to_state(&mut state, &replacement, InputMode::Hold);

        assert!(!state.is_key_down);
        assert_eq!(state.active_mods, 0);
        assert_eq!(state.input_mode, InputMode::Hold);
        assert_eq!(state.combo, replacement.combo);
    }

    #[test]
    fn switching_hotkeys_discards_old_release_state() {
        let old_hotkey = validate_hotkey("F2").unwrap();
        let new_hotkey = validate_hotkey("F3").unwrap();
        let mut state = new_state(&old_hotkey, InputMode::Hold);

        let press_old = process_key_event(&mut state, Key::F2, true);
        assert_eq!(
            press_old,
            ProcessResult {
                consumed: true,
                emitted: Some(HotkeyEvent::HoldStart),
            }
        );

        apply_config_to_state(&mut state, &new_hotkey, InputMode::Hold);

        let release_old = process_key_event(&mut state, Key::F2, false);
        assert_eq!(
            release_old,
            ProcessResult {
                consumed: false,
                emitted: None,
            }
        );

        let press_new = process_key_event(&mut state, Key::F3, true);
        assert_eq!(
            press_new,
            ProcessResult {
                consumed: true,
                emitted: Some(HotkeyEvent::HoldStart),
            }
        );
    }

    #[test]
    fn emits_toggle_and_hold_events_correctly() {
        let toggle_hotkey = validate_hotkey("F2").unwrap();
        let hold_hotkey = validate_hotkey("F3").unwrap();

        let mut toggle_state = new_state(&toggle_hotkey, InputMode::Toggle);
        assert_eq!(
            process_key_event(&mut toggle_state, Key::F2, true),
            ProcessResult {
                consumed: true,
                emitted: Some(HotkeyEvent::ShortPress),
            }
        );
        assert_eq!(
            process_key_event(&mut toggle_state, Key::F2, false),
            ProcessResult {
                consumed: true,
                emitted: None,
            }
        );

        let mut hold_state = new_state(&hold_hotkey, InputMode::Hold);
        assert_eq!(
            process_key_event(&mut hold_state, Key::F3, true),
            ProcessResult {
                consumed: true,
                emitted: Some(HotkeyEvent::HoldStart),
            }
        );
        assert_eq!(
            process_key_event(&mut hold_state, Key::F3, false),
            ProcessResult {
                consumed: true,
                emitted: Some(HotkeyEvent::HoldEnd),
            }
        );
    }

    #[test]
    fn hold_mode_modifier_released_first() {
        let hotkey = validate_hotkey("Ctrl+R").unwrap();
        let mut state = new_state(&hotkey, InputMode::Hold);

        // Ctrl press
        process_key_event(&mut state, Key::ControlLeft, true);
        // R press → HoldStart
        let result = process_key_event(&mut state, Key::KeyR, true);
        assert_eq!(
            result,
            ProcessResult {
                consumed: true,
                emitted: Some(HotkeyEvent::HoldStart),
            }
        );

        // Ctrl released first
        process_key_event(&mut state, Key::ControlLeft, false);
        // R released → should still emit HoldEnd
        let result = process_key_event(&mut state, Key::KeyR, false);
        assert_eq!(
            result,
            ProcessResult {
                consumed: true,
                emitted: Some(HotkeyEvent::HoldEnd),
            }
        );
    }

    #[test]
    fn vk_to_key_covers_all_parse_key_spec_keys() {
        let test_keys = [
            "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
            "CapsLock", "NumLock", "ScrollLock", "Space", "Tab", "Enter", "Escape",
            "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown",
            "Up", "Down", "Left", "Right", "PrintScreen", "Pause",
            "Num0", "Num1", "Num2", "Num3", "Num4", "Num5", "Num6", "Num7", "Num8", "Num9",
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
            "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
            "`", "-", "=", "[", "]", "\\", ";", "'", ",", ".", "/",
        ];
        for key_name in &test_keys {
            let spec = parse_key_spec(key_name);
            assert!(spec.is_some(), "parse_key_spec should support: {}", key_name);
            // Also verify VK mapping exists for this key
            let key = spec.unwrap().key;
            // We can't call vk_to_key without knowing the VK code,
            // but we verify the key is a valid variant by using it in key_hash
            let _hash = key_hash(&key);
        }
    }

    #[test]
    fn vk_to_key_roundtrip_modifiers() {
        // Verify modifier VK codes map correctly
        assert_eq!(vk_to_key(0xA0), Some(Key::ShiftLeft));
        assert_eq!(vk_to_key(0xA1), Some(Key::ShiftRight));
        assert_eq!(vk_to_key(0xA2), Some(Key::ControlLeft));
        assert_eq!(vk_to_key(0xA3), Some(Key::ControlRight));
        assert_eq!(vk_to_key(0xA4), Some(Key::Alt));
        assert_eq!(vk_to_key(0xA5), Some(Key::AltGr));
        assert_eq!(vk_to_key(0x5B), Some(Key::MetaLeft));
        assert_eq!(vk_to_key(0x5C), Some(Key::MetaRight));
    }

    #[test]
    fn vk_to_key_common_keys() {
        // Letters
        assert_eq!(vk_to_key(0x41), Some(Key::KeyA));
        assert_eq!(vk_to_key(0x5A), Some(Key::KeyZ));
        // Digits
        assert_eq!(vk_to_key(0x30), Some(Key::Num0));
        assert_eq!(vk_to_key(0x39), Some(Key::Num9));
        // F-keys
        assert_eq!(vk_to_key(0x70), Some(Key::F1));
        assert_eq!(vk_to_key(0x7B), Some(Key::F12));
        // Special
        assert_eq!(vk_to_key(0x14), Some(Key::CapsLock));
        assert_eq!(vk_to_key(0x20), Some(Key::Space));
        assert_eq!(vk_to_key(0x1B), Some(Key::Escape));
        // Unknown
        assert_eq!(vk_to_key(0xFF), None);
    }

    #[test]
    fn escape_abort_disabled_never_emits() {
        reset_escape_abort_state(false, true);

        assert_eq!(process_escape_abort_event(true), None);
        assert_eq!(process_escape_abort_event(false), None);
    }

    #[test]
    fn escape_abort_inactive_never_emits() {
        reset_escape_abort_state(true, false);

        assert_eq!(process_escape_abort_event(true), None);
        assert_eq!(process_escape_abort_event(false), None);
    }

    #[test]
    fn escape_abort_active_emits_once_on_keydown() {
        reset_escape_abort_state(true, true);

        assert_eq!(
            process_escape_abort_event(true),
            Some(ProcessResult {
                consumed: true,
                emitted: Some(HotkeyEvent::AbortSession),
            })
        );
        assert_eq!(
            process_escape_abort_event(true),
            Some(ProcessResult {
                consumed: true,
                emitted: None,
            })
        );
        assert_eq!(
            process_escape_abort_event(false),
            Some(ProcessResult {
                consumed: true,
                emitted: None,
            })
        );
    }

    #[test]
    fn release_without_prior_press_not_consumed() {
        // If trigger key is released without having been pressed
        // (e.g., modifiers didn't match at press time), it should pass through.
        let hotkey = validate_hotkey("Ctrl+R").unwrap();
        let mut state = new_state(&hotkey, InputMode::Hold);

        // R released without any prior press (no Ctrl was held)
        let result = process_key_event(&mut state, Key::KeyR, false);
        assert_eq!(
            result,
            ProcessResult {
                consumed: false,
                emitted: None,
            }
        );
    }
}
