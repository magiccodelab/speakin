//! Automatic text input — paste or simulate typing into the focused window.

use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// How the transcribed text should be delivered to the active input field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    /// Copy to clipboard and simulate Ctrl+V (fast, reliable).
    Paste,
    /// Simulate Unicode keystroke events (visible typing effect).
    Type,
    /// Do nothing — just show in the app window.
    None,
}

impl OutputMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "paste" => Self::Paste,
            "type" => Self::Type,
            _ => Self::None,
        }
    }
}

/// Send text to the currently focused input field.
pub fn send_text(text: &str, mode: OutputMode) {
    if text.is_empty() {
        return;
    }
    match mode {
        OutputMode::Paste => paste_text(text),
        OutputMode::Type => type_text(text),
        OutputMode::None => {}
    }
}

fn paste_text(text: &str) {
    let Ok(mut clipboard) = Clipboard::new() else {
        log::error!("Failed to open clipboard");
        return;
    };

    // Save old clipboard content
    let old_text = clipboard.get_text().ok();

    // Set new text
    if let Err(e) = clipboard.set_text(text) {
        log::error!("Failed to set clipboard text: {}", e);
        return;
    }

    // Small delay to ensure clipboard is ready
    std::thread::sleep(std::time::Duration::from_millis(30));

    // Simulate Ctrl+V
    let Ok(mut enigo) = Enigo::new(&Settings::default()) else {
        log::error!("Failed to create Enigo instance");
        return;
    };
    let _ = enigo.key(Key::Control, Direction::Press);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = enigo.key(Key::Unicode('v'), Direction::Click);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = enigo.key(Key::Control, Direction::Release);

    // Restore old clipboard after a delay
    if let Some(old) = old_text {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if let Ok(mut cb) = Clipboard::new() {
                let _ = cb.set_text(&old);
            }
        });
    }
}

fn type_text(text: &str) {
    let Ok(mut enigo) = Enigo::new(&Settings::default()) else {
        log::error!("Failed to create Enigo instance");
        return;
    };

    // enigo.text() handles Unicode characters via platform-native input
    if let Err(e) = enigo.text(text) {
        log::error!("Failed to type text: {}", e);
    }
}
