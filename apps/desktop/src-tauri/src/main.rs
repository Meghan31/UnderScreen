// // Prevents additional console window on Windows in release
// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// // Suppress cfg warnings that originate inside the `objc` crate's macros
// #![allow(unexpected_cfgs)]

// use std::sync::atomic::{AtomicBool, Ordering};
// use std::sync::Arc;
// use tauri::Manager;

// // ─────────────────────────────────────────────────────────────────────────────
// // macOS NSWindow → NSPanel overlay configuration
// //
// // CRITICAL CONTEXT — why we go to all this trouble:
// //
// //   The Parakeet / Bartender / CleanShot pattern (a small floating panel that
// //   travels with the user across every Space and into fullscreen apps) does
// //   NOT work reliably with a vanilla NSWindow, no matter which combination of
// //   collectionBehavior bits you set. macOS will quietly pin a regular
// //   NSWindow — especially a borderless, transparent, accessory-app one — to
// //   the Space it was created on.
// //
// //   The fix that the macOS overlay-app community has converged on (see the
// //   tauri-nspanel plugin and the source of every status-bar utility shipped
// //   on macOS): re-class the NSWindow to NSPanel at runtime, add the
// //   NSWindowStyleMaskNonactivatingPanel style mask, and only THEN apply the
// //   collection-behaviour flags. NSPanel is the AppKit class that's actually
// //   designed for floating utility windows that follow the user.
// // ─────────────────────────────────────────────────────────────────────────────

// #[cfg(target_os = "macos")]
// extern "C" {
//     fn object_setClass(
//         obj: *mut objc::runtime::Object,
//         cls: *const objc::runtime::Class,
//     ) -> *const objc::runtime::Class;
// }

// #[cfg(target_os = "macos")]
// fn apply_overlay_window_settings(ns_window: *mut objc::runtime::Object) {
//     use objc::runtime::Class;
//     use objc::{msg_send, sel, sel_impl};

//     unsafe {
//         // ── Step 1: Re-class the NSWindow to NSPanel ──────────────────────
//         // This is the load-bearing fix. After this call, the underlying
//         // AppKit class of the window is NSPanel, which makes the
//         // non-activating-panel style mask (Step 2) and the cross-Space
//         // collection behaviour (Step 5) actually work.
//         if let Some(ns_panel_class) = Class::get("NSPanel") {
//             object_setClass(ns_window, ns_panel_class as *const Class);
//         }

//         // ── Step 2: NSWindowStyleMaskNonactivatingPanel = 1 << 7 = 128 ────
//         // Lets the panel receive clicks WITHOUT making the underscreen app
//         // the frontmost app — your underlying app keeps keyboard focus.
//         // Only meaningful on an NSPanel; that's why Step 1 has to run first.
//         let current_mask: u64 = msg_send![ns_window, styleMask];
//         let _: () = msg_send![ns_window, setStyleMask: current_mask | (1u64 << 7)];

//         // ── Step 3: Stealth — exclude from every macOS screen-capture API ─
//         // NSWindowSharingNone = 0
//         // Excludes the window from ScreenCaptureKit, CGWindowList, QuickTime,
//         // OBS, Zoom, Teams, Meet, Loom — while staying visible on the screen.
//         let _: () = msg_send![ns_window, setSharingType: 0u64];

//         // ── Step 4: Window level ──────────────────────────────────────────
//         // NSStatusWindowLevel = 25.
//         // Above app windows (0), Dock (20), menu bar (24); still participates
//         // in Space management. NOT 1000 (kCGScreenSaverWindowLevel) — at
//         // that level macOS treats the window as a fixed screen-context
//         // overlay and excludes it from Space management entirely.
//         let _: () = msg_send![ns_window, setLevel: 25i64];

//         // ── Step 5: Collection behaviour ──────────────────────────────────
//         //   NSWindowCollectionBehaviorCanJoinAllSpaces    (1 << 0 =   1)
//         //     → window is rendered on EVERY Space simultaneously
//         //   NSWindowCollectionBehaviorIgnoresCycle        (1 << 6 =  64)
//         //     → excluded from Cmd+Tab cycling
//         //   NSWindowCollectionBehaviorFullScreenAuxiliary (1 << 8 = 256)
//         //     → renders alongside fullscreen apps in their Space
//         //
//         // Stationary (1 << 4) is intentionally NOT set — it's for desktop-
//         // tier widgets (wallpaper, dock-style accessories), not overlays.
//         let behavior: u64 = 1 | 64 | 256;
//         let _: () = msg_send![ns_window, setCollectionBehavior: behavior];

//         // ── Step 6: Panel hygiene ─────────────────────────────────────────
//         let _: () = msg_send![ns_window, setHidesOnDeactivate: false];
//         let _: () = msg_send![ns_window, setFloatingPanel: true];
//         let _: () = msg_send![ns_window, setBecomesKeyOnlyIfNeeded: true];
//         let _: () = msg_send![ns_window, setReleasedWhenClosed: false];
//     }
// }

// // ─────────────────────────────────────────────────────────────────────────────
// // IPC Commands
// // ─────────────────────────────────────────────────────────────────────────────

// #[tauri::command]
// fn greet(name: &str) -> String {
//     format!("Hello, {}! UnderScreen is running.", name)
// }

// #[tauri::command]
// fn get_app_status() -> serde_json::Value {
//     serde_json::json!({
//         "status": "running",
//         "version": "0.1.0",
//         "stealth": true,
//         "services": {
//             "overlay": true,
//             "screenshot": false,
//             "llm": false
//         }
//     })
// }

// /// Explicitly set overlay visibility + pointer interactivity from the frontend.
// /// Called when the React UI needs to programmatically control click-through.
// #[tauri::command]
// async fn set_overlay_visible(
//     visible: bool,
//     window: tauri::WebviewWindow,
// ) -> Result<(), String> {
//     if visible {
//         window.show().map_err(|e| e.to_string())?;
//         window.set_ignore_cursor_events(false).map_err(|e| e.to_string())?;
//     } else {
//         window.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
//         window.hide().map_err(|e| e.to_string())?;
//     }
//     Ok(())
// }

// // ─────────────────────────────────────────────────────────────────────────────
// // Entry point
// // ─────────────────────────────────────────────────────────────────────────────

// fn main() {
//     // Shared toggle state — true = overlay is currently visible / interactive
//     let overlay_visible = Arc::new(AtomicBool::new(true));

//     tauri::Builder::default()
//         // ── Plugins ───────────────────────────────────────────────────────
//         .plugin(tauri_plugin_opener::init())
//         .plugin(tauri_plugin_global_shortcut::Builder::new().build())
//         // ── IPC handlers ──────────────────────────────────────────────────
//         .invoke_handler(tauri::generate_handler![
//             greet,
//             get_app_status,
//             set_overlay_visible,
//         ])
//         // ── Setup hook (runs after the window is created) ─────────────────
//         .setup(move |app| {
//             let window = app
//                 .get_webview_window("main")
//                 .expect("main window not found");

//             // ── macOS-specific: stealth + activation policy ───────────────
//             #[cfg(target_os = "macos")]
//             {
//                 use tauri::ActivationPolicy;

//                 // LSUIElement equivalent at runtime:
//                 // • No Dock icon
//                 // • No app name in the menu bar
//                 // • Excluded from Cmd+Tab switcher
//                 //
//                 // This is necessary on top of the NSPanel re-class — a
//                 // Regular-policy app's windows can be tied to the app's
//                 // current Space context, which can override the
//                 // CanJoinAllSpaces collection-behaviour flag.
//                 app.set_activation_policy(ActivationPolicy::Accessory);

//                 // ns_window() returns Result<*mut c_void, _> — unwrap then cast
//                 let ns_window_ptr = window
//                     .ns_window()
//                     .expect("failed to get NSWindow pointer");
//                 let ns_window = ns_window_ptr as *mut objc::runtime::Object;
//                 apply_overlay_window_settings(ns_window);

//                 // ⚠️ Intentionally NOT calling Tauri's
//                 // window.set_visible_on_all_workspaces(true) or
//                 // window.set_always_on_top(true) here. Those helpers
//                 // call AppKit's setCollectionBehavior: and setLevel:
//                 // with their own values and CLOBBER the NSPanel-aware
//                 // configuration we just applied above.

//                 // ── Safety net: re-apply on every focus event ─────────
//                 // macOS occasionally resets collectionBehavior during
//                 // Space transitions, hide/show cycles, and activation-
//                 // context changes. Re-applying on focus events guarantees
//                 // the panel returns to the correct state automatically.
//                 let win_clone = window.clone();
//                 window.on_window_event(move |event| {
//                     if let tauri::WindowEvent::Focused(_) = event {
//                         if let Ok(ptr) = win_clone.ns_window() {
//                             apply_overlay_window_settings(
//                                 ptr as *mut objc::runtime::Object,
//                             );
//                         }
//                     }
//                 });
//             }

//             // ── Position window: right side of primary monitor ────────────
//             // Computed at runtime so it works on any screen resolution /
//             // Retina scale factor. Places the panel 20px from the right
//             // edge and 28px from the top (just below the macOS menu bar).
//             {
//                 let panel_width  = 440.0_f64;
//                 let margin_right = 20.0_f64;
//                 let margin_top   = 28.0_f64; // clear the macOS menu bar

//                 if let Ok(Some(monitor)) = window.primary_monitor() {
//                     let size  = monitor.size();         // physical pixels
//                     let scale = monitor.scale_factor(); // 2.0 on Retina

//                     // Convert to logical (AppKit / CSS) coordinates
//                     let logical_w = size.width  as f64 / scale;
//                     let logical_h = size.height as f64 / scale;

//                     let x = (logical_w - panel_width - margin_right).max(0.0);
//                     let y = margin_top;

//                     // Clamp panel height so it fits on any screen
//                     let panel_height = 760.0_f64.min(logical_h - margin_top - 20.0);

//                     let _ = window.set_size(tauri::LogicalSize::new(panel_width, panel_height));
//                     let _ = window.set_position(tauri::LogicalPosition::new(x, y));
//                 }
//             }

//             // ── Global hotkey: Cmd+Shift+Space ────────────────────────────
//             // First press  → show overlay + enable pointer events (interactive mode)
//             // Second press → restore click-through + hide overlay (stealth mode)
//             {
//                 use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

//                 let toggle_window = window.clone();
//                 let toggle_state = overlay_visible.clone();

//                 app.handle()
//                     .global_shortcut()
//                     .on_shortcut("CmdOrCtrl+Shift+Space", move |_app, _shortcut, event| {
//                         if event.state() == ShortcutState::Pressed {
//                             // Atomically flip the flag and read the OLD value
//                             let was_visible = toggle_state.fetch_xor(true, Ordering::SeqCst);

//                             if was_visible {
//                                 // Transition: visible → hidden (stealth mode)
//                                 let _ = toggle_window.set_ignore_cursor_events(true);
//                                 let _ = toggle_window.hide();
//                             } else {
//                                 // Transition: hidden → visible (interactive mode)
//                                 let _ = toggle_window.show();
//                                 let _ = toggle_window.set_ignore_cursor_events(false);
//                                 // Raise to front without stealing keyboard focus from the user's app
//                                 let _ = toggle_window.set_focus();
//                             }
//                         }
//                     })
//                     .expect("failed to register global shortcut Cmd+Shift+Space");
//             }

//             // Default: overlay is visible but clicks pass through.
//             // The user sees it; their underlying app still receives all input.
//             let _ = window.set_ignore_cursor_events(true);

//             Ok(())
//         })
//         .run(tauri::generate_context!())
//         .expect("error while running tauri application");
// }



// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Suppress cfg warnings that originate inside the `objc` crate's macros
#![allow(unexpected_cfgs)]

mod gemini_llm;
mod screenshot;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;

// ─────────────────────────────────────────────────────────────────────────────
// Shared state: which snap position the panel is currently anchored to.
//
// "top-*"    → panel grows DOWN as the answer streams in (top edge is anchored)
// "bottom-*" → panel grows UP   as the answer streams in (bottom edge is anchored)
//
// We need to remember this between hotkey presses so resize_window() can keep
// the correct edge pinned when the panel grows or shrinks.
// ─────────────────────────────────────────────────────────────────────────────
struct SnapState(Mutex<String>);

// ─────────────────────────────────────────────────────────────────────────────
// macOS NSWindow → NSPanel overlay configuration
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
extern "C" {
    fn object_setClass(
        obj: *mut objc::runtime::Object,
        cls: *const objc::runtime::Class,
    ) -> *const objc::runtime::Class;
}

#[cfg(target_os = "macos")]
fn apply_overlay_window_settings(ns_window: *mut objc::runtime::Object) {
    use objc::runtime::Class;
    use objc::{msg_send, sel, sel_impl};

    unsafe {
        // ── Step 1: Re-class NSWindow → NSPanel ───────────────────────────
        // Load-bearing fix. NSPanel is the AppKit class designed for floating
        // utility windows. Without this, CanJoinAllSpaces is silently ignored
        // on fullscreen Space transitions.
        if let Some(ns_panel_class) = Class::get("NSPanel") {
            object_setClass(ns_window, ns_panel_class as *const Class);
        }

        // ── Step 2: NonactivatingPanel style mask (1 << 7 = 128) ─────────
        // Panel receives clicks WITHOUT making UnderScreen the frontmost app.
        // Must run after Step 1 — only meaningful on NSPanel.
        let current_mask: u64 = msg_send![ns_window, styleMask];
        let _: () = msg_send![ns_window, setStyleMask: current_mask | (1u64 << 7)];

        // ── Step 3: Exclude from all screen-capture APIs ──────────────────
        // NSWindowSharingNone = 0
        // Invisible to ScreenCaptureKit, CGWindowList, Zoom, OBS, etc.
        let _: () = msg_send![ns_window, setSharingType: 0u64];

        // ── Step 4: Window level = NSStatusWindowLevel (25) ───────────────
        // Floats above normal app windows and the Dock, but stays below
        // system UI. Participates in Space management (unlike level 1000).
        let _: () = msg_send![ns_window, setLevel: 25i64];

        // ── Step 5: Collection behaviour ──────────────────────────────────
        // NSWindowCollectionBehaviorCanJoinAllSpaces    (1 <<  0 =   1)
        //   → rendered on every Space and inside fullscreen apps simultaneously
        // NSWindowCollectionBehaviorIgnoresCycle        (1 <<  6 =  64)
        //   → excluded from Cmd+Tab / Mission Control cycling
        // NSWindowCollectionBehaviorFullScreenAuxiliary (1 <<  8 = 256)
        //   → renders alongside fullscreen apps (not just normal Spaces)
        //
        // NOTE: macOS resets these silently on Space transitions — that is
        // why apply_overlay_window_settings() is called from the window-event
        // hook on EVERY focus event, not just at startup.
        let _: () = msg_send![ns_window, setCollectionBehavior: 1u64 | 64u64 | 256u64];

        // ── Step 6: Panel hygiene ─────────────────────────────────────────
        let _: () = msg_send![ns_window, setHidesOnDeactivate:     false];
        let _: () = msg_send![ns_window, setFloatingPanel:          true];
        let _: () = msg_send![ns_window, setBecomesKeyOnlyIfNeeded: true];
        let _: () = msg_send![ns_window, setReleasedWhenClosed:     false];
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Window positioning helper
// ─────────────────────────────────────────────────────────────────────────────

/// Snap the window to one of six named positions on the primary monitor.
///
/// Positions: "top-left" | "top-right" | "top-middle"
///            "bottom-left" | "bottom-right" | "bottom-middle"
///
/// For bottom-* positions we use the panel's CURRENT inner height (not the
/// max-allowed 390 px) so the bottom edge actually sits on the screen edge —
/// otherwise a compact 185-px panel snapped to "bottom-right" lands hundreds
/// of pixels above the bottom (i.e. roughly the middle of the screen).
///
/// resize_window() re-runs this same anchoring math whenever the panel grows
/// or shrinks, so the bottom edge stays pinned and the panel grows UPWARD as
/// the answer streams in.
fn position_window(window: &tauri::WebviewWindow, pos: &str) {
    let panel_width: f64  = 210.0; // ≈ 5 cm — matches CSS + resize_window
    let margin:      f64  = 12.0;
    let margin_top:  f64  = 28.0; // clear the macOS menu bar

    let Ok(Some(monitor)) = window.primary_monitor() else { return };
    let size  = monitor.size();
    let scale = monitor.scale_factor();
    let logical_w = size.width  as f64 / scale;
    let logical_h = size.height as f64 / scale;

    // Use the actual current panel height for bottom anchoring.
    // Fall back to 185 (compact idle height) if inner_size is unavailable.
    let actual_h = window
        .inner_size()
        .ok()
        .map(|s| s.height as f64 / scale)
        .unwrap_or(185.0);

    let (x, y): (f64, f64) = match pos {
        "top-left"      => (margin,                                  margin_top),
        "top-right"     => (logical_w - panel_width - margin,        margin_top),
        "top-middle"    => ((logical_w - panel_width) / 2.0,         margin_top),
        "bottom-left"   => (margin,                                  logical_h - actual_h - margin),
        "bottom-right"  => (logical_w - panel_width - margin,        logical_h - actual_h - margin),
        "bottom-middle" => ((logical_w - panel_width) / 2.0,         logical_h - actual_h - margin),
        _ => return,
    };

    let _ = window.set_position(tauri::LogicalPosition::new(x, y));
}

// ─────────────────────────────────────────────────────────────────────────────
// IPC Commands
// ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! UnderScreen is running.", name)
}

#[tauri::command]
fn get_app_status() -> serde_json::Value {
    serde_json::json!({
        "status": "running",
        "version": "0.1.0",
        "stealth": true,
        "services": {
            "overlay": true,
            "screenshot": false,
            "llm": false
        }
    })
}

/// Quit the app — called from the React close button in the drag header.
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// Resize the window height to match the panel content.
/// Called from React's ResizeObserver whenever the panel grows or shrinks.
/// Width is fixed; only height changes.
///
/// If the panel is currently snapped to a "bottom-*" position, we ALSO
/// reposition it so the bottom edge stays pinned to its anchor — this is what
/// makes the panel grow UPWARD as the answer streams in instead of growing
/// downward off the bottom of the screen. For "top-*" positions we leave the
/// y coordinate alone, so the panel naturally grows downward.
#[tauri::command]
async fn resize_window(
    height: f64,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, SnapState>,
) -> Result<(), String> {
    // 210 logical px ≈ 5 cm at 96 dpi — matches the fixed panel width in CSS.
    const PANEL_W: f64 = 210.0;
    const MARGIN:  f64 = 12.0;
    // Clamp: never smaller than 150 px, never taller than 390 px (~10 cm).
    let h = height.max(150.0).min(390.0);

    window
        .set_size(tauri::LogicalSize::new(PANEL_W, h))
        .map_err(|e| e.to_string())?;

    // Re-anchor the bottom edge if we're snapped to a bottom-* position.
    let pos = state.0.lock().map(|p| p.clone()).unwrap_or_default();
    if pos.starts_with("bottom-") {
        if let Ok(Some(monitor)) = window.primary_monitor() {
            let size  = monitor.size();
            let scale = monitor.scale_factor();
            let logical_w = size.width  as f64 / scale;
            let logical_h = size.height as f64 / scale;

            let new_y = logical_h - h - MARGIN;
            let new_x = match pos.as_str() {
                "bottom-left"   => MARGIN,
                "bottom-right"  => logical_w - PANEL_W - MARGIN,
                "bottom-middle" => (logical_w - PANEL_W) / 2.0,
                _ => return Ok(()),
            };
            let _ = window.set_position(tauri::LogicalPosition::new(new_x, new_y));
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    // true  = overlay is currently visible (always click-through)
    // false = overlay is hidden
    //
    // The overlay is NEVER interactive via mouse. All interaction is hotkey-only.
    // set_ignore_cursor_events(true) is a permanent, unconditional invariant.
    let overlay_visible = Arc::new(AtomicBool::new(true));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // Initial snap position must match the startup placement below
        // (top-right corner). resize_window() reads this to decide whether to
        // re-anchor the bottom edge when the panel grows.
        .manage(SnapState(Mutex::new("top-right".to_string())))
        .invoke_handler(tauri::generate_handler![greet, get_app_status, quit_app, resize_window])
        .setup(move |app| {
            let window = app
                .get_webview_window("main")
                .expect("main window not found");

            // ── macOS: activation policy + NSPanel setup ──────────────────
            #[cfg(target_os = "macos")]
            {
                use tauri::ActivationPolicy;

                // No Dock icon, no menu-bar name, excluded from Cmd+Tab.
                app.set_activation_policy(ActivationPolicy::Accessory);

                // Initial NSPanel configuration.
                let ns_window_ptr = window.ns_window().expect("failed to get NSWindow pointer");
                let ns_window = ns_window_ptr as *mut objc::runtime::Object;
                apply_overlay_window_settings(ns_window);

                // ── Re-apply on every window event ────────────────────────
                //
                // macOS silently resets collectionBehavior AND ignoresCursorEvents
                // on Space transitions (4-finger swipes, fullscreen app switches).
                // Re-asserting on Focused is the standard recovery, but we also
                // re-assert on every other event as a belt-and-suspenders measure
                // so there is never a window between "Space entered" and
                // "click-through restored" where the overlay can accidentally
                // capture input.
                let win_clone = window.clone();
                window.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::Focused(_) => {
                            // Re-apply NSPanel settings (collection behaviour, level, etc.)
                            if let Ok(ptr) = win_clone.ns_window() {
                                apply_overlay_window_settings(
                                    ptr as *mut objc::runtime::Object,
                                );
                            }
                            // Keep the visible overlay interactive after focus.
                            let _ = win_clone.set_ignore_cursor_events(false);
                        }
                        // Re-assert on move/resize (catches show() side-effects).
                        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                            let _ = win_clone.set_ignore_cursor_events(false);
                        }
                        _ => {}
                    }
                });
            }

            // ── Initial position + size ───────────────────────────────────
            // Window starts compact (≈ 5 cm × 5 cm).  React's ResizeObserver
            // will call resize_window() to grow/shrink the height dynamically.
            {
                const PANEL_W: f64    = 210.0; // ≈ 5 cm
                const PANEL_H: f64    = 185.0; // compact idle height
                const MARGIN:  f64    = 12.0;
                const MENU_BAR: f64   = 28.0;

                if let Ok(Some(monitor)) = window.primary_monitor() {
                    let size  = monitor.size();
                    let scale = monitor.scale_factor();
                    let logical_w = size.width as f64 / scale;

                    let x = (logical_w - PANEL_W - MARGIN).max(0.0);
                    let y = MENU_BAR;

                    let _ = window.set_size(tauri::LogicalSize::new(PANEL_W, PANEL_H));
                    let _ = window.set_position(tauri::LogicalPosition::new(x, y));
                }
            }

            // ── Global hotkey: Cmd+Shift+Space ────────────────────────────
            //
            //   VISIBLE → press → HIDDEN    (overlay disappears entirely)
            //   HIDDEN  → press → VISIBLE   (overlay reappears, interactive)
            //
            //   The visible overlay should accept pointer input.
            {
                use tauri::Emitter;
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

                let toggle_window = window.clone();
                let toggle_state  = overlay_visible.clone();

                app.handle()
                    .global_shortcut()
                    .on_shortcut("CmdOrCtrl+Shift+Space", move |_app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            // Atomically flip the flag; fetch_xor returns the OLD value.
                            let was_visible = toggle_state.fetch_xor(true, Ordering::SeqCst);

                            if was_visible {
                                // VISIBLE → HIDDEN
                                // Restore click-through before hiding.
                                let _ = toggle_window.set_ignore_cursor_events(true);
                                let _ = toggle_window.hide();
                                // Notify React so its mode state stays in sync.
                                let _ = toggle_window.emit(
                                    "overlay-toggle",
                                    serde_json::json!({ "visible": false }),
                                );
                            } else {
                                // HIDDEN → VISIBLE
                                // Show the window and keep it interactive.
                                let _ = toggle_window.show();
                                let _ = toggle_window.set_ignore_cursor_events(false);

                                // Re-apply NSPanel settings — macOS may have reset them
                                // while the window was hidden or during the Space transition.
                                #[cfg(target_os = "macos")]
                                if let Ok(ptr) = toggle_window.ns_window() {
                                    apply_overlay_window_settings(
                                        ptr as *mut objc::runtime::Object,
                                    );
                                }

                                // Notify React so its mode state stays in sync.
                                let _ = toggle_window.emit(
                                    "overlay-toggle",
                                    serde_json::json!({ "visible": true }),
                                );
                            }
                        }
                    })
                    .expect("failed to register global shortcut Cmd+Shift+Space");
            }

            // ── Global hotkey: Cmd+Shift+S — full pipeline ───────────────
            //
            // Flow (fully async, no UI interaction required):
            //   1. Show overlay if hidden (so the user can see progress).
            //   2. Emit "capture-start"  → React: status = scanning
            //   3. [blocking] Screenshot + Apple Vision OCR
            //   4. Emit "llm-start"      → React: status = thinking, clear answer
            //   5. Stream OpenAI response as "llm-token" events → React: appends tokens
            //   6. Emit "llm-done"       → React: status = done
            //      OR "pipeline-error"  → React: status = error
            //
            // contentProtected = true means our window is excluded from the
            // ScreenCaptureKit frame — the screenshot captures only the user's
            // content behind the overlay.
            {
                use tauri::Emitter;
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

                let pipeline_window  = window.clone();
                let pipeline_visible = overlay_visible.clone();

                app.handle()
                    .global_shortcut()
                    .on_shortcut("CmdOrCtrl+Shift+S", move |_app, _shortcut, event| {
                        if event.state() != ShortcutState::Pressed {
                            return;
                        }

                        // Always show the overlay so the user can see progress.
                        if !pipeline_visible.load(Ordering::SeqCst) {
                            let _ = pipeline_window.show();
                            let _ = pipeline_window.set_ignore_cursor_events(true);
                            pipeline_visible.store(true, Ordering::SeqCst);

                            #[cfg(target_os = "macos")]
                            if let Ok(ptr) = pipeline_window.ns_window() {
                                apply_overlay_window_settings(
                                    ptr as *mut objc::runtime::Object,
                                );
                            }
                        }

                        // Notify React: pipeline starting.
                        let _ = pipeline_window.emit("capture-start", ());

                        // Spawn the full async pipeline on Tauri's runtime.
                        let win = pipeline_window.clone();
                        tauri::async_runtime::spawn(async move {
                            // ── Step 1: Screenshot + OCR (blocking) ──────────
                            let ocr = match tokio::task::spawn_blocking(
                                screenshot::capture_and_ocr,
                            )
                            .await
                            {
                                Ok(Ok(r))  => r,
                                Ok(Err(e)) => {
                                    let _ = win.emit("pipeline-error", &e);
                                    return;
                                }
                                Err(e) => {
                                    let _ = win.emit("pipeline-error", &e.to_string());
                                    return;
                                }
                            };

                            // ── Step 2: LLM streaming ─────────────────────────
                            match gemini_llm::query_streaming(&ocr.ocr_text, &win).await {
                                Ok(_)  => { let _ = win.emit("llm-done", ()); }
                                Err(e) => { let _ = win.emit("pipeline-error", &e); }
                            }
                        });
                    })
                    .expect("failed to register global shortcut Cmd+Shift+S");
            }

            // ── Global hotkeys: snap window to 6 positions ───────────────
            //
            // Cmd+Ctrl+Option+Left        → top-left
            // Cmd+Ctrl+Option+Right       → top-right
            // Cmd+Ctrl+Option+Up          → top-middle
            // Cmd+Ctrl+Option+Down        → bottom-middle
            // Cmd+Ctrl+Option+Shift+Left  → bottom-left
            // Cmd+Ctrl+Option+Shift+Right → bottom-right
            //
            // Note: "two arrow keys simultaneously" (e.g. Left+Down) is not
            // supported by any OS-level hotkey API, so Shift differentiates the
            // bottom-corner positions from the top-corner ones.
            {
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

                let snap_pairs: &[(&str, &str)] = &[
                    ("CmdOrCtrl+Alt+ArrowLeft",        "top-left"),
                    ("CmdOrCtrl+Alt+ArrowRight",       "top-right"),
                    ("CmdOrCtrl+Alt+ArrowUp",          "top-middle"),
                    ("CmdOrCtrl+Alt+ArrowDown",        "bottom-middle"),
                    ("CmdOrCtrl+Alt+Shift+ArrowLeft",  "bottom-left"),
                    ("CmdOrCtrl+Alt+Shift+ArrowRight", "bottom-right"),
                ];

                for (hotkey, snap_pos) in snap_pairs {
                    let snap_window = window.clone();
                    let pos = snap_pos.to_string();

                    app.handle()
                        .global_shortcut()
                        .on_shortcut(*hotkey, move |app_handle, _shortcut, event| {
                            if event.state() == ShortcutState::Pressed {
                                // Remember the new snap position so resize_window()
                                // knows which edge to keep anchored when the
                                // panel grows or shrinks.
                                if let Ok(mut s) = app_handle.state::<SnapState>().0.lock() {
                                    *s = pos.clone();
                                }
                                position_window(&snap_window, &pos);
                            }
                        })
                        .unwrap_or_else(|e| eprintln!("Failed to register snap hotkey {hotkey}: {e}"));
                }
            }

            // ── Global hotkeys: quit ────────────────────────────────────
            {
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

                app.handle()
                    .global_shortcut()
                    .on_shortcut("CmdOrCtrl+Shift+Q", move |handle, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            handle.exit(0);
                        }
                    })
                    .expect("failed to register global shortcut Cmd+Shift+Q");

                app.handle()
                    .global_shortcut()
                    .on_shortcut("CmdOrCtrl+Alt+X", move |handle, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            handle.exit(0);
                        }
                    })
                    .expect("failed to register global shortcut Cmd+Option+X");
            }

            // ── Boot state: visible and interactive ────────────────────────
            let _ = window.set_ignore_cursor_events(false);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}