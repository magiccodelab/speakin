// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Hidden CLI entry used by the NSIS PreUninstall hook to scrub OS keyring
    // credentials before the installed binary is removed. Handled BEFORE any
    // Tauri initialization so no window/tray/hotkey side effects happen during
    // uninstall. See storage::uninstall_cleanup for the safety contract.
    #[cfg(windows)]
    if std::env::args().skip(1).any(|a| a == "--uninstall-cleanup") {
        speakin_lib::run_uninstall_cleanup();
        return;
    }

    speakin_lib::run()
}
