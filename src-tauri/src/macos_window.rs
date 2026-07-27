//! Native macOS window behavior that cannot be represented in Tauri's JSON config.
//!
//! Wardian uses an overlay titlebar so its 36px chrome can share a row with the
//! standard traffic lights. AppKit normally turns the green control into a
//! separate full-screen Space, which removes that titlebar. Explicitly opting
//! out restores its conventional zoom behavior and keeps the controls in the
//! application window.

use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
use tauri::{Manager, Runtime};

/// Makes the main window's green traffic light zoom the window instead of
/// entering a separate macOS full-screen Space.
///
/// This is deliberately applied after Tauri creates the native window. The
/// Overlay configuration remains responsible for the traffic-light inset.
pub fn configure_main_window<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    // `ns_window` is owned by Tauri for the lifetime of the WebviewWindow and
    // setup runs on the native application thread. The borrowed AppKit handle
    // is used only for this synchronous configuration call.
    let ns_window: &NSWindow = unsafe { &*window.ns_window()?.cast() };
    let mut behavior = ns_window.collectionBehavior();
    behavior.remove(
        NSWindowCollectionBehavior::FullScreenPrimary
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    behavior.insert(NSWindowCollectionBehavior::FullScreenNone);
    ns_window.setCollectionBehavior(behavior);

    Ok(())
}
