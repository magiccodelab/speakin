//! Global hotkey support using `rdev` low-level keyboard hooks.
//!
//! Uses `SetWindowsHookEx(WH_KEYBOARD_LL)` on Windows via rdev, which
//! supports ALL keys including CapsLock, NumLock, ScrollLock, etc.
//! Unlike `RegisterHotKey`, low-level hooks can detect both press and release.
//!
//! Uses `rdev::grab()` (not `listen()`) so that matched hotkey events are
//! consumed and NOT forwarded to the system. This prevents CapsLock from
//! toggling caps state, etc.

use parking_lot::Mutex;
use rdev::{self, EventType, Key};
use std::sync::OnceLock;
use std::time::Instant;

/// Hotkey events sent to the main app logic.
#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    /// Short press detected (toggle mode): start or stop recording
    ShortPress,
    /// Long press started (hold mode): start recording
    HoldStart,
    /// Long press ended (hold mode): stop recording
    HoldEnd,
}

/// Input mode determines how the hotkey behaves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Toggle,
    Hold,
}

// ── Modifier flags ──

const MOD_CTRL: u8 = 0x01;
const MOD_ALT: u8 = 0x02;
const MOD_SHIFT: u8 = 0x04;

/// A hotkey combination: optional modifiers + a trigger key.
#[derive(Debug, Clone, Copy)]
struct HotkeyCombo {
    modifiers: u8,
    key: Key,
}

/// Internal state for the hotkey handler.
struct HotkeyState {
    is_key_down: bool,
    _key_down_time: Option<Instant>,
    input_mode: InputMode,
    combo: HotkeyCombo,
    /// Track which modifier keys are currently pressed.
    active_mods: u8,
}

static HOTKEY_STATE: OnceLock<Mutex<HotkeyState>> = OnceLock::new();
static HOTKEY_TX: OnceLock<std::sync::mpsc::Sender<HotkeyEvent>> = OnceLock::new();

/// Map a key name string to an rdev Key.
fn key_name_to_rdev(name: &str) -> Option<Key> {
    match name {
        // Function keys
        "F1" => Some(Key::F1),
        "F2" => Some(Key::F2),
        "F3" => Some(Key::F3),
        "F4" => Some(Key::F4),
        "F5" => Some(Key::F5),
        "F6" => Some(Key::F6),
        "F7" => Some(Key::F7),
        "F8" => Some(Key::F8),
        "F9" => Some(Key::F9),
        "F10" => Some(Key::F10),
        "F11" => Some(Key::F11),
        "F12" => Some(Key::F12),
        // Toggle keys (these DON'T work with RegisterHotKey, but work here)
        "CapsLock" | "CAPSLOCK" | "Caps" => Some(Key::CapsLock),
        "NumLock" | "NUMLOCK" => Some(Key::NumLock),
        "ScrollLock" | "SCROLLLOCK" => Some(Key::ScrollLock),
        // Navigation
        "Space" | "SPACE" => Some(Key::Space),
        "Tab" | "TAB" => Some(Key::Tab),
        "Enter" | "ENTER" => Some(Key::Return),
        "Escape" | "ESC" => Some(Key::Escape),
        "Backspace" | "BACKSPACE" => Some(Key::Backspace),
        "Delete" | "DELETE" => Some(Key::Delete),
        "Insert" | "INSERT" => Some(Key::Insert),
        "Home" | "HOME" => Some(Key::Home),
        "End" | "END" => Some(Key::End),
        "PageUp" | "PAGEUP" => Some(Key::PageUp),
        "PageDown" | "PAGEDOWN" => Some(Key::PageDown),
        "Up" => Some(Key::UpArrow),
        "Down" => Some(Key::DownArrow),
        "Left" => Some(Key::LeftArrow),
        "Right" => Some(Key::RightArrow),
        "PrintScreen" | "PrtSc" => Some(Key::PrintScreen),
        "Pause" | "PAUSE" => Some(Key::Pause),
        // Single letter A-Z
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic() => {
            rdev_key_from_char(s.as_bytes()[0].to_ascii_uppercase() as char)
        }
        // Digits 0-9
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_digit() => {
            rdev_key_from_char(s.as_bytes()[0] as char)
        }
        // Numpad
        "Num0" => Some(Key::Kp0),
        "Num1" => Some(Key::Kp1),
        "Num2" => Some(Key::Kp2),
        "Num3" => Some(Key::Kp3),
        "Num4" => Some(Key::Kp4),
        "Num5" => Some(Key::Kp5),
        "Num6" => Some(Key::Kp6),
        "Num7" => Some(Key::Kp7),
        "Num8" => Some(Key::Kp8),
        "Num9" => Some(Key::Kp9),
        // Punctuation
        "`" => Some(Key::BackQuote),
        "-" => Some(Key::Minus),
        "=" => Some(Key::Equal),
        "[" => Some(Key::LeftBracket),
        "]" => Some(Key::RightBracket),
        "\\" => Some(Key::BackSlash),
        ";" => Some(Key::SemiColon),
        "'" => Some(Key::Quote),
        "," => Some(Key::Comma),
        "." => Some(Key::Dot),
        "/" => Some(Key::Slash),
        _ => None,
    }
}

fn rdev_key_from_char(ch: char) -> Option<Key> {
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

/// Check if an rdev Key is a modifier key.
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

/// Parse a hotkey string like "Ctrl+Shift+V", "CapsLock", or "F2".
fn parse_hotkey(name: &str) -> HotkeyCombo {
    let parts: Vec<&str> = name.split('+').map(|s| s.trim()).collect();
    let mut modifiers: u8 = 0;
    let mut key = Key::F2; // default fallback

    for part in &parts {
        match part.to_uppercase().as_str() {
            "CTRL" | "CONTROL" => modifiers |= MOD_CTRL,
            "ALT" => modifiers |= MOD_ALT,
            "SHIFT" => modifiers |= MOD_SHIFT,
            _ => {
                if let Some(k) = key_name_to_rdev(part) {
                    key = k;
                }
            }
        }
    }

    HotkeyCombo { modifiers, key }
}

/// Start the global hotkey listener on a dedicated thread.
pub fn start_listener(
    hotkey_name: &str,
    input_mode: InputMode,
) -> std::sync::mpsc::Receiver<HotkeyEvent> {
    let (tx, rx) = std::sync::mpsc::channel();
    let combo = parse_hotkey(hotkey_name);

    HOTKEY_TX.get_or_init(|| tx);
    HOTKEY_STATE.get_or_init(|| {
        Mutex::new(HotkeyState {
            is_key_down: false,
            _key_down_time: None,
            input_mode,
            combo,
            active_mods: 0,
        })
    });

    // Also update in case already initialized
    {
        let mut state = HOTKEY_STATE.get().unwrap().lock();
        state.input_mode = input_mode;
        state.combo = combo;
    }

    std::thread::spawn(move || {
        log::info!("Starting rdev grab (hotkey events will be consumed)");
        if let Err(e) = rdev::grab(grab_callback) {
            log::error!("rdev grab error: {:?}", e);
        }
    });

    rx
}

/// Update the hotkey configuration at runtime.
pub fn update_config(hotkey_name: &str, input_mode: InputMode) {
    if let Some(state) = HOTKEY_STATE.get() {
        let mut s = state.lock();
        s.combo = parse_hotkey(hotkey_name);
        s.input_mode = input_mode;
    }
}

/// rdev grab callback — intercepts keyboard events.
/// Returns `None` to consume matched hotkey events (prevents system handling),
/// returns `Some(event)` to pass through all other events.
fn grab_callback(event: rdev::Event) -> Option<rdev::Event> {
    match event.event_type {
        EventType::KeyPress(key) => {
            if handle_key_event(key, true) {
                None // Consumed — don't forward to system
            } else {
                Some(event) // Not our hotkey — pass through
            }
        }
        EventType::KeyRelease(key) => {
            if handle_key_event(key, false) {
                None // Consumed — don't forward to system
            } else {
                Some(event)
            }
        }
        _ => Some(event), // Mouse events etc. — always pass through
    }
}

/// Handle a key event. Returns `true` if the event was consumed as part of
/// our hotkey (caller should suppress it), `false` if it should pass through.
fn handle_key_event(key: Key, is_press: bool) -> bool {
    // Update modifier tracking — always pass modifiers through
    if is_modifier_key(&key) {
        let flag = modifier_flag(&key);
        if let Some(state_lock) = HOTKEY_STATE.get() {
            let mut state = state_lock.lock();
            if is_press {
                state.active_mods |= flag;
            } else {
                state.active_mods &= !flag;
            }
        }
        return false; // Don't consume modifier keys
    }

    let Some(state_lock) = HOTKEY_STATE.get() else {
        return false;
    };
    let mut state = state_lock.lock();

    // Check if this key matches our trigger
    if !keys_match(&key, &state.combo.key) {
        return false;
    }

    // Check modifiers match
    if state.active_mods != state.combo.modifiers {
        return false;
    }

    // Hotkey matched — handle event and consume it
    let input_mode = state.input_mode;
    let was_down = state.is_key_down;

    if is_press {
        if was_down {
            return true; // Key repeat — still consume but don't fire event
        }
        state.is_key_down = true;
        state._key_down_time = Some(Instant::now());
        drop(state);

        match input_mode {
            InputMode::Hold => {
                if let Some(tx) = HOTKEY_TX.get() {
                    let _ = tx.send(HotkeyEvent::HoldStart);
                }
            }
            InputMode::Toggle => {
                if let Some(tx) = HOTKEY_TX.get() {
                    let _ = tx.send(HotkeyEvent::ShortPress);
                }
            }
        }
    } else {
        // Release: reset state and optionally send HoldEnd
        state.is_key_down = false;
        state._key_down_time = None;
        drop(state);

        if input_mode == InputMode::Hold {
            if let Some(tx) = HOTKEY_TX.get() {
                let _ = tx.send(HotkeyEvent::HoldEnd);
            }
        }
    }

    true // Event consumed
}

/// Compare two rdev Keys for equality.
fn keys_match(a: &Key, b: &Key) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}
