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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Manager;

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
        .invoke_handler(tauri::generate_handler![greet, get_app_status])
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
                            // Re-assert click-through immediately after focus.
                            // macOS can reset ignoresCursorEvents during activation.
                            let _ = win_clone.set_ignore_cursor_events(true);
                        }
                        // Re-assert on move/resize (catches show() side-effects).
                        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                            let _ = win_clone.set_ignore_cursor_events(true);
                        }
                        _ => {}
                    }
                });
            }

            // ── Position: right side of primary monitor ───────────────────
            {
                let panel_width  = 440.0_f64;
                let margin_right = 20.0_f64;
                let margin_top   = 28.0_f64; // clear the macOS menu bar

                if let Ok(Some(monitor)) = window.primary_monitor() {
                    let size  = monitor.size();
                    let scale = monitor.scale_factor();

                    let logical_w = size.width  as f64 / scale;
                    let logical_h = size.height as f64 / scale;

                    let x = (logical_w - panel_width - margin_right).max(0.0);
                    let y = margin_top;
                    let panel_height = 760.0_f64.min(logical_h - margin_top - 20.0);

                    let _ = window.set_size(tauri::LogicalSize::new(panel_width, panel_height));
                    let _ = window.set_position(tauri::LogicalPosition::new(x, y));
                }
            }

            // ── Global hotkey: Cmd+Shift+Space ────────────────────────────
            //
            //   VISIBLE → press → HIDDEN    (overlay disappears entirely)
            //   HIDDEN  → press → VISIBLE   (overlay reappears, click-through)
            //
            //   The overlay is NEVER interactive via mouse in either state.
            //   set_ignore_cursor_events(true) is always set after show().
            {
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
                                let _ = toggle_window.set_ignore_cursor_events(true);
                                let _ = toggle_window.hide();
                            } else {
                                // HIDDEN → VISIBLE
                                // show() first so the window exists on-screen, then
                                // IMMEDIATELY lock click-through before any mouse event
                                // can land on the freshly-shown window.
                                let _ = toggle_window.show();
                                let _ = toggle_window.set_ignore_cursor_events(true);

                                // Re-apply NSPanel settings — macOS may have reset them
                                // while the window was hidden or during the Space transition.
                                #[cfg(target_os = "macos")]
                                if let Ok(ptr) = toggle_window.ns_window() {
                                    apply_overlay_window_settings(
                                        ptr as *mut objc::runtime::Object,
                                    );
                                }
                            }
                        }
                    })
                    .expect("failed to register global shortcut Cmd+Shift+Space");
            }

            // ── Boot state: visible, permanently click-through ────────────
            // Every pointer event falls through to whatever app is behind the
            // overlay. This is an unconditional invariant — it is never lifted.
            let _ = window.set_ignore_cursor_events(true);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}