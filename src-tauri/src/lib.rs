//! Orbi desktop shell — a small always-on-top face at the top of the screen
//! that shows what an AI agent is doing and lets the user answer its
//! permission requests from anywhere.
//!
//! One window, "widget" (see `tauri.conf.json`): transparent, always-on-top,
//! click-through except while the cursor is over it. Rust owns the window's
//! geometry; the frontend owns what is drawn inside it.

mod byok;
mod explain;
mod http;
mod integrations;
mod server;
mod settings;

use std::{fs, path::PathBuf, sync::Mutex};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Position, Size,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Raises the widget's NSWindow to `NSStatusWindowLevel` (the level menu-bar
/// status items use). macOS clamps ordinary windows to the visible frame,
/// which excludes the menu bar; at this level y=0 is honoured.
#[cfg(target_os = "macos")]
fn raise_above_menu_bar(window: &tauri::WebviewWindow) {
    use objc2_app_kit::{NSStatusWindowLevel, NSWindow};
    let Ok(ptr) = window.ns_window() else { return };
    // SAFETY: `ns_window()` returns the live NSWindow backing this window,
    // which is alive for the duration of `setup()` where this is called.
    let ns_window: &NSWindow = unsafe { &*(ptr as *mut NSWindow) };
    ns_window.setLevel(NSStatusWindowLevel);
    // On every Space, including over full-screen apps, and not swept along
    // by Mission Control. Without this the face stays behind on whichever
    // Space it launched on.
    {
        use objc2_app_kit::NSWindowCollectionBehavior as B;
        ns_window.setCollectionBehavior(
            B::CanJoinAllSpaces | B::Stationary | B::FullScreenAuxiliary | B::IgnoresCycle,
        );
    }
    // Debug builds only: `ORBI_ALLOW_CAPTURE=1` keeps the face in screenshots
    // so it can be checked visually during development.
    let allow_capture =
        cfg!(debug_assertions) && std::env::var_os("ORBI_ALLOW_CAPTURE").is_some();
    if !allow_capture {
        exclude_from_screen_capture(ns_window);
    }
}

/// Keeps the face out of screen recordings and shares — it can show commands
/// and file paths the user would not want broadcast.
#[cfg(target_os = "macos")]
fn exclude_from_screen_capture(ns_window: &objc2_app_kit::NSWindow) {
    #[allow(deprecated)]
    ns_window.setSharingType(objc2_app_kit::NSWindowSharingType::None);
}

#[cfg(not(target_os = "macos"))]
fn raise_above_menu_bar(_window: &tauri::WebviewWindow) {}

/// Brings Orbi to the front so a window can take keystrokes. As an
/// `Accessory` app, `set_focus()` alone raises the window without moving key
/// focus.
#[cfg(target_os = "macos")]
pub(crate) fn activate_app() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    let Some(mtm) = MainThreadMarker::new() else { return };
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn activate_app() {}

/// Stops macOS App Nap from freezing the face's animation. An `Accessory` app
/// is never frontmost, so without this the webview stops painting. The token
/// is leaked on purpose: releasing it would end the activity.
#[cfg(target_os = "macos")]
fn prevent_app_nap() {
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
    let reason = NSString::from_str("Orbi's face animates while visible");
    let token = NSProcessInfo::processInfo()
        .beginActivityWithOptions_reason(NSActivityOptions::UserInitiated, &reason);
    std::mem::forget(token);
}

#[cfg(not(target_os = "macos"))]
fn prevent_app_nap() {}

/// Global cursor position in logical screen points (top-left origin).
#[cfg(target_os = "macos")]
fn cursor_position() -> (f64, f64) {
    use objc2_core_graphics::{CGEvent, CGEventSource, CGEventSourceStateID};
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
    match CGEvent::new(source.as_deref()) {
        Some(event) => {
            let p = CGEvent::location(Some(&event));
            (p.x, p.y)
        }
        None => (0.0, 0.0),
    }
}

#[cfg(not(target_os = "macos"))]
fn cursor_position() -> (f64, f64) {
    (0.0, 0.0)
}

const WIDGET_WINDOW: &str = "widget";

/// Global hotkeys. Control+Option pairs, so they never collide with typing.
const APPROVE_SHORTCUT: &str = "control+alt+KeyA";
const DENY_SHORTCUT: &str = "control+alt+KeyD";
const EXPAND_SHORTCUT: &str = "control+alt+KeyO";
const JUMP_SHORTCUT: &str = "control+alt+KeyJ";

/// Whether the widget is expanded. Rust owns the toggle and emits the
/// absolute value (not a bare "toggle" signal) so a double-registered
/// listener in React StrictMode stays idempotent.
struct WidgetExpanded(Mutex<bool>);

/// Whether the layout editor is open. While true, nothing else repositions
/// the window.
struct WidgetEditMode(Mutex<bool>);

/// The user's saved width and window position, held in memory because every
/// geometry path needs it, not just launch.
#[derive(Default, Clone, Copy)]
struct SavedGeometry {
    width: Option<f64>,
    x: Option<f64>,
    y: Option<f64>,
}
struct LayoutState(Mutex<SavedGeometry>);

/// Window sizes, including transparent room for glow to bleed into.
const WIDGET_COLLAPSED: (f64, f64) = (320.0, 104.0);

/// Flush with the top edge — possible because of `raise_above_menu_bar`.
const MARGIN_TOP: f64 = 0.0;

/// Resizes to `(w, h)` and recentres horizontally at `MARGIN_TOP`, so the face
/// stays anchored top-centre as it grows.
fn set_widget_size_centered(app: &AppHandle, w: f64, h: f64) {
    let Some(widget) = app.get_webview_window(WIDGET_WINDOW) else { return };
    let _ = widget.set_size(Size::Logical(LogicalSize::new(w, h)));
    let Ok(Some(monitor)) = widget.current_monitor() else { return };
    let screen = monitor.size();
    let scale = monitor.scale_factor();
    let x = ((screen.width as f64 / scale) - w) / 2.0;
    let _ = widget.set_position(Position::Logical(LogicalPosition::new(x, MARGIN_TOP)));
}

fn position_widget(app: &AppHandle) {
    let (w, h) = collapsed_size(app);
    place_widget(app, w, h);
}

/// Collapsed size, honouring a saved width. Height follows the width so the
/// face never distorts.
fn collapsed_size(app: &AppHandle) -> (f64, f64) {
    let width = app
        .try_state::<LayoutState>()
        .and_then(|s| s.0.lock().ok().and_then(|g| g.width))
        .unwrap_or(WIDGET_COLLAPSED.0);
    (width, width * (WIDGET_COLLAPSED.1 / WIDGET_COLLAPSED.0))
}

/// The single owner of widget geometry: saved position if there is one,
/// otherwise top-centre. A no-op while the layout editor is open.
fn place_widget(app: &AppHandle, w: f64, h: f64) {
    let editing = app
        .try_state::<WidgetEditMode>()
        .and_then(|s| s.0.lock().ok().map(|g| *g))
        .unwrap_or(false);
    if editing {
        return;
    }

    let saved = app
        .try_state::<LayoutState>()
        .and_then(|s| s.0.lock().ok().map(|g| *g))
        .unwrap_or_default();
    match (saved.x, saved.y) {
        (Some(x), Some(y)) => {
            let Some(widget) = app.get_webview_window(WIDGET_WINDOW) else { return };
            let _ = widget.set_size(Size::Logical(LogicalSize::new(w, h)));
            // A spot saved on a bigger display must not strand the face off-screen.
            let (sw, sh) = screen_size_of(app);
            let x = x.clamp(0.0, (sw - w).max(0.0));
            let y = y.clamp(0.0, (sh - 60.0).max(0.0));
            let _ = widget.set_position(Position::Logical(LogicalPosition::new(x, y)));
        }
        _ => set_widget_size_centered(app, w, h),
    }
}

/// Expands/collapses the preview in place and emits `widget-expanded`.
/// Deliberately never steals focus: the preview is read-only, and answering
/// happens through global hotkeys.
fn toggle_widget(app: &AppHandle) {
    let expanded = {
        let state = app.state::<WidgetExpanded>();
        let mut expanded = state.0.lock().unwrap();
        *expanded = !*expanded;
        *expanded
    };
    // No resize here: the frontend measures its content and calls
    // `fit_widget_height`, so the window is never briefly the wrong size.
    let _ = app.emit_to(WIDGET_WINDOW, "widget-expanded", expanded);
}

/// Force-collapses. Idempotent.
#[tauri::command]
fn collapse_widget(app: AppHandle) {
    *app.state::<WidgetExpanded>().0.lock().unwrap() = false;
    let (w, h) = collapsed_size(&app);
    place_widget(&app, w, h);
    let _ = app.emit_to(WIDGET_WINDOW, "widget-expanded", false);
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn get_cursor_position() -> (f64, f64) {
    cursor_position()
}

/// Cursor position relative to the face (−1..1 from its centre), how
/// frantically it's moving, and whether it's over the widget.
#[derive(serde::Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Gaze {
    x: f64,
    y: f64,
    agitation: u8,
    hovering: bool,
}

/// How long the cursor must sit still before the eyes stop tracking it.
const GAZE_IDLE_MS: u128 = 8_000;

/// The most recent gaze, so a freshly loaded webview can pull it instead of
/// waiting for a change event that may never come.
struct LastGaze(Mutex<Gaze>);

#[tauri::command]
fn get_gaze(app: AppHandle) -> Gaze {
    app.try_state::<LastGaze>()
        .and_then(|s| s.0.lock().ok().map(|g| *g))
        .unwrap_or(Gaze { x: 0.0, y: 0.0, agitation: 0, hovering: false })
}

/// Distance at which gaze reaches half its range. A hyperbolic falloff
/// (`d / (|d| + SOFTNESS)`) never plateaus, so the eyes keep tracking
/// anywhere on screen; a hard clamp saturated and froze them.
const GAZE_SOFTNESS: f64 = 180.0;

/// How far outside the widget the cursor may be while it still accepts
/// clicks, so a click landing on the same frame as the pointer isn't lost.
const CLICK_MARGIN: f64 = 24.0;

/// The 60Hz native cursor loop. Lives in Rust because the webview only sees
/// mouse events over its own small window. Emits `cursor-gaze` only when the
/// quantised value changes, and toggles click-through only when the cursor
/// crosses the widget boundary.
fn spawn_gaze_loop(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_sent: Option<Gaze> = None;
        let mut last_pos = (0.0_f64, 0.0_f64);
        let mut last_move = std::time::Instant::now();
        // Recent horizontal directions, for reversal counting.
        let mut dirs: Vec<i8> = Vec::with_capacity(16);
        // Window frame, re-read about once a second: each read is a
        // main-thread round trip, and the window rarely moves.
        let mut frame: Option<(f64, f64, f64, f64)> = None;
        let mut frame_age = 0u32;
        let mut last_ignore: Option<bool> = None;

        loop {
            std::thread::sleep(std::time::Duration::from_millis(16));

            let Some(window) = app.get_webview_window(WIDGET_WINDOW) else { continue };
            // An error reading visibility is not the same as hidden.
            if let Ok(false) = window.is_visible() {
                continue;
            }

            frame_age += 1;
            if frame.is_none() || frame_age >= 60 {
                frame_age = 0;
                if let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) {
                    // A failed scale factor must not silently become 1.0.
                    if let Ok(scale) = window.scale_factor() {
                        let (ww, wh) = (size.width as f64 / scale, size.height as f64 / scale);
                        if ww > 0.0 && wh > 0.0 {
                            frame = Some((pos.x as f64 / scale, pos.y as f64 / scale, ww, wh));
                        }
                    }
                }
            }
            let Some((wx, wy, ww, wh)) = frame else { continue };

            let (cx, cy) = cursor_position();
            let moved = (cx - last_pos.0).abs() > 1.0 || (cy - last_pos.1).abs() > 1.0;

            if moved {
                let dir: i8 = if cx > last_pos.0 { 1 } else if cx < last_pos.0 { -1 } else { 0 };
                if dir != 0 {
                    if dirs.last().copied().unwrap_or(0) != dir {
                        dirs.push(dir);
                    }
                    if dirs.len() > 8 {
                        dirs.remove(0);
                    }
                }
                last_pos = (cx, cy);
                last_move = std::time::Instant::now();
            } else if last_move.elapsed().as_millis() > 700 {
                dirs.clear();
            }

            let (fx, fy) = (wx + ww / 2.0, wy + wh / 2.0);
            let hovering = cx >= wx && cx <= wx + ww && cy >= wy && cy <= wy + wh;

            // Click-through unless the cursor is near the widget, so the face
            // never blocks clicks on whatever sits under the top of the
            // screen. Never click-through while expanded or in edit mode.
            {
                let interactive = app
                    .try_state::<WidgetExpanded>()
                    .and_then(|s| s.0.lock().ok().map(|g| *g))
                    .unwrap_or(false)
                    || app
                        .try_state::<WidgetEditMode>()
                        .and_then(|s| s.0.lock().ok().map(|g| *g))
                        .unwrap_or(false);

                let near = cx >= wx - CLICK_MARGIN
                    && cx <= wx + ww + CLICK_MARGIN
                    && cy >= wy - CLICK_MARGIN
                    && cy <= wy + wh + CLICK_MARGIN;

                let want_ignore = !interactive && !near;
                if last_ignore != Some(want_ignore) {
                    last_ignore = Some(want_ignore);
                    let _ = window.set_ignore_cursor_events(want_ignore);
                }
            }

            let soften = |d: f64| d / (d.abs() + GAZE_SOFTNESS);
            let idle = last_move.elapsed().as_millis() > GAZE_IDLE_MS;
            let (gx, gy) = if idle { (0.0, 0.0) } else { (soften(cx - fx), soften(cy - fy)) };

            let gaze = Gaze {
                // ~1/60th steps: fine enough to keep tracking, coarse enough
                // to ignore sensor jitter.
                x: (gx * 60.0).round() / 60.0,
                y: (gy * 60.0).round() / 60.0,
                agitation: dirs.len().saturating_sub(2).min(3) as u8,
                hovering,
            };

            if let Some(state) = app.try_state::<LastGaze>() {
                if let Ok(mut g) = state.0.lock() {
                    *g = gaze;
                }
            }

            if last_sent != Some(gaze) {
                last_sent = Some(gaze);
                let _ = app.emit("cursor-gaze", gaze);
            }
        }
    });
}

/// Opens the layout editor (the editor has its own Cancel/Save).
#[tauri::command]
fn enter_widget_edit_mode(app: AppHandle) {
    *app.state::<WidgetEditMode>().0.lock().unwrap() = true;
    if let Some(widget) = app.get_webview_window(WIDGET_WINDOW) {
        activate_app();
        let _ = widget.set_focus();
        let _ = widget.emit("widget-edit-mode", true);
    }
}

#[tauri::command]
fn exit_widget_edit_mode(app: AppHandle) {
    *app.state::<WidgetEditMode>().0.lock().unwrap() = false;
}

/// Logical size of the display the face lives on (primary as a fallback).
fn screen_size_of(app: &AppHandle) -> (f64, f64) {
    let monitor = app
        .get_webview_window(WIDGET_WINDOW)
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    monitor
        .map(|m| {
            let s = m.size();
            (s.width as f64 / m.scale_factor(), s.height as f64 / m.scale_factor())
        })
        .unwrap_or((1512.0, 982.0))
}

/// The camera housing on this Mac's built-in display, in points: its width and
/// how far it reaches down. `None` on displays without one.
#[derive(serde::Serialize, Clone, Copy, Debug)]
struct Notch {
    w: f64,
    h: f64,
}

#[cfg(target_os = "macos")]
fn notch_now() -> Option<Notch> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    let mtm = MainThreadMarker::new()?;
    for screen in NSScreen::screens(mtm).iter() {
        let top = screen.safeAreaInsets().top;
        if top <= 0.0 {
            continue;
        }
        let frame = screen.frame();
        let w = frame.size.width - screen.auxiliaryTopLeftArea().size.width - screen.auxiliaryTopRightArea().size.width;
        if w > 0.0 {
            return Some(Notch { w, h: top });
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn notch_now() -> Option<Notch> {
    None
}

/// So the face can hang *below* the camera instead of hiding behind it.
#[tauri::command]
fn get_notch(app: AppHandle) -> Option<Notch> {
    // AppKit wants the main thread; hop there if this command isn't on it.
    if let Some(n) = notch_now() {
        return Some(n);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(notch_now());
    })
    .ok()?;
    rx.recv_timeout(std::time::Duration::from_millis(500)).ok().flatten()
}

/// For the Appearance page's drag-to-place map.
#[tauri::command]
fn get_screen_size(app: AppHandle) -> (f64, f64) {
    screen_size_of(&app)
}

fn layout_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("layout.json"))
}

/// Saves the layout verbatim. The schema lives in `src/lib/layout.ts`; Rust
/// only reads back width and position.
#[tauri::command]
fn save_layout(app: AppHandle, window: tauri::WebviewWindow, layout: serde_json::Value) -> Result<(), String> {
    settings::from_settings(&window)?;
    let path = layout_path(&app)?;
    let pretty = serde_json::to_string_pretty(&layout).map_err(|e| e.to_string())?;
    fs::write(path, pretty + "\n").map_err(|e| e.to_string())?;
    apply_saved_layout(&app);
    // The face re-reads its size and length live.
    let _ = app.emit_to(WIDGET_WINDOW, "layout-changed", layout);
    Ok(())
}

#[tauri::command]
fn load_layout(app: AppHandle) -> Option<serde_json::Value> {
    let path = layout_path(&app).ok()?;
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

#[tauri::command]
fn reset_layout(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    settings::from_settings(&window)?;
    let path = layout_path(&app)?;
    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    if let Some(state) = app.try_state::<LayoutState>() {
        if let Ok(mut g) = state.0.lock() {
            *g = SavedGeometry::default();
        }
    }
    position_widget(&app);
    let _ = app.emit_to(WIDGET_WINDOW, "layout-changed", serde_json::Value::Null);
    Ok(())
}

/// Grows or shrinks the widget to fit what the frontend laid out. Lives in
/// Rust because a `resizable: false` window ignores the JS `setSize` on macOS.
#[tauri::command]
fn fit_widget_height(app: AppHandle, height: f64) {
    let (w, collapsed_h) = collapsed_size(&app);
    let max_h = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.size().height as f64 / m.scale_factor())
        .unwrap_or(900.0);
    place_widget(&app, w, height.clamp(collapsed_h, max_h - 40.0));
}

/// Seeds `LayoutState` from `layout.json` and places the widget. A missing or
/// corrupt file falls back to top-centre.
fn apply_saved_layout(app: &AppHandle) {
    if let Some(layout) = load_layout(app.clone()) {
        if let Some(state) = app.try_state::<LayoutState>() {
            if let Ok(mut g) = state.0.lock() {
                g.width = layout.get("width").and_then(|v| v.as_f64());
                g.x = layout.get("x").and_then(|v| v.as_f64());
                g.y = layout.get("y").and_then(|v| v.as_f64());
            }
        }
    }
    position_widget(app);
}

/// The tray's "Pause Orbi" check item, kept so pausing from Settings can
/// update it too.
struct PauseItem(CheckMenuItem<tauri::Wry>);

/// Pause/resume from anywhere (tray, Settings), keeping both in sync.
pub(crate) fn set_paused(app: &AppHandle, paused: bool) {
    server::set_paused(app, paused);
    if let Some(item) = app.try_state::<PauseItem>() {
        let _ = item.0.set_checked(paused);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch (double-clicking the app again, a
        // login item racing a manual start) just opens Settings in the
        // running Orbi instead of starting a rival server and hotkey set.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            settings::open_settings_window(app);
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let is = |s: &str| s.parse::<Shortcut>().ok() == Some(*shortcut);
                    if is(APPROVE_SHORTCUT) {
                        server::answer(app, server::Decision::Allow);
                    } else if is(DENY_SHORTCUT) {
                        server::answer(app, server::Decision::Deny);
                    } else if is(EXPAND_SHORTCUT) {
                        toggle_widget(app);
                    } else if is(JUMP_SHORTCUT) {
                        server::jump_to_agent(app);
                    }
                })
                .build(),
        )
        .manage(WidgetExpanded(Mutex::new(false)))
        .manage(WidgetEditMode(Mutex::new(false)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(server::CoreState::new())
        .manage(server::TokenState(Mutex::new(String::new())))
        .manage(settings::ConfigState(Mutex::new(settings::Config::default())))
        .manage(LayoutState(Mutex::new(SavedGeometry::default())))
        .manage(LastGaze(Mutex::new(Gaze { x: 0.0, y: 0.0, agitation: 0, hovering: false })))
        .invoke_handler(tauri::generate_handler![
            collapse_widget,
            save_layout,
            load_layout,
            reset_layout,
            get_screen_size,
            get_notch,
            enter_widget_edit_mode,
            exit_widget_edit_mode,
            fit_widget_height,
            quit_app,
            get_gaze,
            get_cursor_position,
            server::get_state,
            settings::get_settings,
            settings::update_settings,
            settings::set_paused,
            settings::set_api_key,
            settings::test_explain,
            settings::list_integrations,
            settings::connect_integration,
            settings::preview_integration,
            settings::disconnect_integration,
            settings::regenerate_token,
            settings::get_app_info,
            settings::open_settings
        ])
        .setup(|app| {
            // Menu-bar app: no Dock icon, no Cmd-Tab entry.
            #[cfg(target_os = "macos")]
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            // `Accessory` makes this process a permanent App Nap candidate.
            prevent_app_nap();

            if cfg!(debug_assertions) {
                eprintln!("[orbi] notch: {:?}", notch_now());
            }
            apply_saved_layout(app.handle());
            spawn_gaze_loop(app.handle().clone());
            if let Some(widget) = app.get_webview_window(WIDGET_WINDOW) {
                raise_above_menu_bar(&widget);
            }

            server::start(app.handle().clone());
            settings::init(app.handle());

            // A hotkey another app already owns must not stop Orbi launching;
            // the tray still works without it.
            for key in [APPROVE_SHORTCUT, DENY_SHORTCUT, EXPAND_SHORTCUT, JUMP_SHORTCUT] {
                if let Err(e) = app.global_shortcut().register(key) {
                    eprintln!("[orbi] could not register {key}: {e}");
                }
            }

            // Shortcuts are OS-level global hotkeys, so they appear here as
            // label hints, not menu accelerators (which would double-register).
            let pause_item =
                CheckMenuItem::with_id(app, "pause", "Pause Orbi", true, false, None::<&str>)?;
            let approve_item =
                MenuItem::with_id(app, "approve", "Approve request   ⌃⌥A", true, None::<&str>)?;
            let deny_item =
                MenuItem::with_id(app, "deny", "Deny request   ⌃⌥D", true, None::<&str>)?;
            let expand_item =
                MenuItem::with_id(app, "expand", "Expand preview   ⌃⌥O", true, None::<&str>)?;
            let jump_item =
                MenuItem::with_id(app, "jump", "Jump to agent   ⌃⌥J", true, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Orbi", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(
                app,
                &[
                    &approve_item, &deny_item, &expand_item, &jump_item, &sep1,
                    &pause_item, &settings_item, &sep2, &quit_item,
                ],
            )?;

            // Template image: macOS tints it to match the menu bar.
            let tray_icon =
                tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))
                    .expect("tray template icon must decode");

            let pause_handle = pause_item.clone();
            app.manage(PauseItem(pause_item.clone()));
            TrayIconBuilder::new()
                .icon_as_template(true)
                .icon(tray_icon)
                .tooltip("Orbi")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "pause" => {
                        let paused = pause_handle.is_checked().unwrap_or(false);
                        set_paused(app, paused);
                        let _ = app.emit("settings-changed", settings::get_settings(app.clone()));
                    }
                    "approve" => {
                        server::answer(app, server::Decision::Allow);
                    }
                    "deny" => {
                        server::answer(app, server::Decision::Deny);
                    }
                    "expand" => toggle_widget(app),
                    "jump" => server::jump_to_agent(app),
                    "settings" => settings::open_settings_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Orbi")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                server::remove_port_file();
            }
        });
}
