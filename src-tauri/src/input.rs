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
pub fn send_text(text: &str, mode: OutputMode, copy_to_clipboard: bool, restore_clipboard: bool) {
    if text.is_empty() {
        return;
    }
    match mode {
        OutputMode::Paste => {
            // In paste mode, skip restore if user wants text in clipboard OR restore is disabled
            let should_restore = !copy_to_clipboard && restore_clipboard;
            paste_text(text, should_restore);
        }
        OutputMode::Type => {
            type_text(text);
            if copy_to_clipboard {
                copy_text_to_clipboard(text);
            }
        }
        OutputMode::None => {
            if copy_to_clipboard {
                copy_text_to_clipboard(text);
            }
        }
    }
}

fn copy_text_to_clipboard(text: &str) {
    if let Ok(mut clipboard) = Clipboard::new() {
        if let Err(e) = clipboard.set_text(text) {
            log::error!("Failed to copy text to clipboard: {}", e);
        }
    } else {
        log::error!("Failed to open clipboard for copy");
    }
}

/// Saved clipboard content for restoration after paste.
enum SavedClipboard {
    Text(String),
    Image(arboard::ImageData<'static>),
    /// Clipboard was empty or unsupported format — clear after paste to avoid residue.
    ClearAfterPaste,
    /// Restoration disabled — do nothing.
    Skip,
}

/// Max image size to save for restoration (~10 MB RGBA). Larger images are skipped
/// to avoid adding noticeable latency before the Ctrl+V keystroke.
const MAX_IMAGE_RESTORE_BYTES: usize = 10 * 1024 * 1024;
const CLIPBOARD_RESTORE_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

fn paste_text(text: &str, should_restore: bool) {
    let Ok(mut clipboard) = Clipboard::new() else {
        log::error!("Failed to open clipboard");
        return;
    };

    // Save old clipboard content (text or image)
    let saved = if should_restore {
        if let Ok(t) = clipboard.get_text() {
            if !t.is_empty() {
                SavedClipboard::Text(t)
            } else if let Ok(img) = clipboard.get_image() {
                // get_text() can return Ok("") when clipboard has image — try image
                let size = img.bytes.len();
                if size <= MAX_IMAGE_RESTORE_BYTES {
                    SavedClipboard::Image(img.to_owned_img())
                } else {
                    log::debug!("Skipping image clipboard restore ({} bytes > limit)", size);
                    SavedClipboard::ClearAfterPaste
                }
            } else {
                SavedClipboard::ClearAfterPaste
            }
        } else if let Ok(img) = clipboard.get_image() {
            let size = img.bytes.len();
            if size <= MAX_IMAGE_RESTORE_BYTES {
                SavedClipboard::Image(img.to_owned_img())
            } else {
                log::debug!("Skipping image clipboard restore ({} bytes > limit)", size);
                SavedClipboard::ClearAfterPaste
            }
        } else {
            SavedClipboard::ClearAfterPaste
        }
    } else {
        SavedClipboard::Skip
    };

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
    match saved {
        SavedClipboard::Text(old) => {
            let pasted = text.to_string();
            std::thread::spawn(move || {
                std::thread::sleep(CLIPBOARD_RESTORE_DELAY);
                if let Ok(mut cb) = Clipboard::new() {
                    // Only restore if clipboard still contains our pasted text;
                    // if the user copied something new in the meantime, don't overwrite it.
                    if let Ok(current) = cb.get_text() {
                        if current == pasted {
                            let _ = cb.set_text(&old);
                        }
                    }
                }
            });
        }
        SavedClipboard::Image(img) => {
            let pasted = text.to_string();
            std::thread::spawn(move || {
                std::thread::sleep(CLIPBOARD_RESTORE_DELAY);
                if let Ok(mut cb) = Clipboard::new() {
                    if let Ok(current) = cb.get_text() {
                        if current == pasted {
                            if let Err(e) = cb.set_image(img) {
                                log::debug!("Failed to restore image clipboard: {}", e);
                            }
                        }
                    }
                }
            });
        }
        SavedClipboard::ClearAfterPaste => {
            // Old clipboard was empty or unsupported — clear our pasted text to avoid residue
            let pasted = text.to_string();
            std::thread::spawn(move || {
                std::thread::sleep(CLIPBOARD_RESTORE_DELAY);
                if let Ok(mut cb) = Clipboard::new() {
                    if let Ok(current) = cb.get_text() {
                        if current == pasted {
                            cb.clear().unwrap_or_default();
                        }
                    }
                }
            });
        }
        SavedClipboard::Skip => {}
    }
}

fn type_text(text: &str) {
    let Ok(mut enigo) = Enigo::new(&Settings::default()) else {
        log::error!("Failed to create Enigo instance");
        return;
    };

    // Sanitize control characters: replace newlines with spaces (enigo interprets
    // \n as Enter), remove other ASCII control chars except tab (useful for code).
    let sanitized: String = text
        .chars()
        .filter_map(|c| {
            if c == '\n' || c == '\r' {
                Some(' ')
            } else if c.is_control() && c != '\t' {
                None
            } else {
                Some(c)
            }
        })
        .collect();

    if let Err(e) = enigo.text(&sanitized) {
        log::error!("Failed to type text: {}", e);
    }
}
