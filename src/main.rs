//! Rust Taskbar: a safe-to-close floating taskbar prototype for Windows 11.
//! Explorer remains the system shell and its taskbar stays intact.
#![cfg_attr(windows, windows_subsystem = "windows")]
// Treat accidentally unbound Win32 message constants as build errors.
#![deny(unreachable_patterns, unused_variables)]

mod model;

#[cfg(windows)]
mod windows_app;

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_app::run() {
        // A GUI-subsystem application has no console in normal use.
        // Show startup errors without modifying Windows settings.
        windows_app::show_error(&error);
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("rust-taskbar targets Windows 11 only");
}
