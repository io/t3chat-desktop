use std::fs;
use std::path::PathBuf;

use tauri::{plugin::TauriPlugin, Runtime, Url};
use tauri_plugin_opener::open_url;

fn is_t3_chat_host(host: &str) -> bool {
    host == "t3.chat" || host.ends_with(".t3.chat")
}

const BG_CACHE_FILE_NAME: &str = "last-bg-color.txt";
const WINDOW_STATE_FILE_NAME: &str = "window-state.txt";

#[derive(Clone, Copy, Default)]
struct StoredWindowState {
    zoom: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
    x: Option<i32>,
    y: Option<i32>,
}

fn color_to_rgba_css(color: tauri::window::Color) -> String {
    format!(
        "rgba({}, {}, {}, {:.6})",
        color.0,
        color.1,
        color.2,
        color.3 as f32 / 255.0
    )
}

fn bg_cache_path<R: Runtime>(manager: &impl tauri::Manager<R>) -> Option<PathBuf> {
    manager
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join(BG_CACHE_FILE_NAME))
}

fn window_state_path<R: Runtime>(manager: &impl tauri::Manager<R>) -> Option<PathBuf> {
    manager
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join(WINDOW_STATE_FILE_NAME))
}

fn parse_window_state(raw: &str) -> StoredWindowState {
    let mut state = StoredWindowState::default();

    for line in raw.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "zoom" => state.zoom = value.parse::<f64>().ok(),
            "width" => state.width = value.parse::<f64>().ok(),
            "height" => state.height = value.parse::<f64>().ok(),
            "x" => state.x = value.parse::<i32>().ok(),
            "y" => state.y = value.parse::<i32>().ok(),
            _ => {}
        }
    }

    state
}

fn serialize_window_state(state: StoredWindowState) -> String {
    let mut lines = Vec::new();
    if let Some(zoom) = state.zoom {
        lines.push(format!("zoom={zoom:.6}"));
    }
    if let Some(width) = state.width {
        lines.push(format!("width={width:.2}"));
    }
    if let Some(height) = state.height {
        lines.push(format!("height={height:.2}"));
    }
    if let Some(x) = state.x {
        lines.push(format!("x={x}"));
    }
    if let Some(y) = state.y {
        lines.push(format!("y={y}"));
    }
    lines.join("\n")
}

fn load_cached_window_state<R: Runtime>(manager: &impl tauri::Manager<R>) -> StoredWindowState {
    let Some(path) = window_state_path(manager) else {
        return StoredWindowState::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return StoredWindowState::default();
    };
    parse_window_state(&raw)
}

fn save_cached_window_state<R: Runtime>(
    manager: &impl tauri::Manager<R>,
    state: StoredWindowState,
) {
    let Some(path) = window_state_path(manager) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, serialize_window_state(state));
}

fn save_cached_zoom_level<R: Runtime>(manager: &impl tauri::Manager<R>, zoom: f64) {
    let mut state = load_cached_window_state(manager);
    state.zoom = Some(zoom.clamp(0.5, 2.0));
    save_cached_window_state(manager, state);
}

fn current_logical_size<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Option<(f64, f64)> {
    let size = window.inner_size().ok()?;
    let scale_factor = window.scale_factor().ok()?;
    let logical_size = size.to_logical::<f64>(scale_factor);
    Some((logical_size.width, logical_size.height))
}

fn save_cached_window_size<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let Some((width, height)) = current_logical_size(window) else {
        return;
    };
    let mut state = load_cached_window_state(window);
    state.width = Some(width.max(1.0));
    state.height = Some(height.max(1.0));
    save_cached_window_state(window, state);
}

fn save_cached_window_position<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let Ok(position) = window.outer_position() else {
        return;
    };
    let mut state = load_cached_window_state(window);
    state.x = Some(position.x);
    state.y = Some(position.y);
    save_cached_window_state(window, state);
}

fn load_cached_bg_color<R: Runtime>(
    manager: &impl tauri::Manager<R>,
) -> Option<tauri::window::Color> {
    let path = bg_cache_path(manager)?;
    let value = fs::read_to_string(path).ok()?;
    parse_css_color(value.trim())
}

fn save_cached_bg_color<R: Runtime>(manager: &impl tauri::Manager<R>, color: tauri::window::Color) {
    let Some(path) = bg_cache_path(manager) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, color_to_rgba_css(color));
}

const BG_PRELOAD_SCRIPT: &str = r#"
(() => {
  try {
    const stored = (localStorage.getItem('t3chat.desktopBgColor') || '').trim();
    if (!/^rgba?\(/i.test(stored)) return;

    document.documentElement.style.backgroundColor = stored;
    const style = document.createElement('style');
    style.id = 't3chat-preload-bg';
    style.textContent = `html, body { background-color: ${stored} !important; }`;
    (document.head || document.documentElement).appendChild(style);
  } catch {}
})();
"#;

#[cfg(target_os = "macos")]
fn setup_macos_app_menu<R: Runtime>(
    app_handle: &tauri::AppHandle<R>,
    zoom_level: std::sync::Arc<std::sync::Mutex<f64>>,
) -> tauri::Result<()> {
    use std::sync::Arc;
    use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
    use tauri::Manager;

    let config = app_handle.config();
    let package = app_handle.package_info();
    let app_name = config
        .product_name
        .clone()
        .unwrap_or_else(|| package.name.clone());

    let about_metadata = AboutMetadata {
        name: Some(app_name.clone()),
        version: Some(package.version.to_string()),
        credits: Some(
            "Independent desktop wrapper for t3.chat\n\
            Website: https://t3.chat\n\
            \n\
            Community-made project. Not affiliated with, endorsed by, or sponsored by the t3.chat team."
                .to_string(),
        ),
        copyright: config.bundle.copyright.clone(),
        authors: config
            .bundle
            .publisher
            .clone()
            .map(|publisher| vec![publisher]),
        icon: app_handle.default_window_icon().cloned(),
        ..Default::default()
    };

    let zoom_in_item =
        MenuItem::with_id(app_handle, "zoom-in", "Zoom In", true, Some("Cmd+Shift+="))?;
    let zoom_out_item = MenuItem::with_id(app_handle, "zoom-out", "Zoom Out", true, Some("Cmd+-"))?;
    let zoom_reset_item =
        MenuItem::with_id(app_handle, "zoom-reset", "Actual Size", true, None::<&str>)?;
    let window_menu = Submenu::with_items(
        app_handle,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app_handle, None)?,
            &PredefinedMenuItem::maximize(app_handle, None)?,
            &PredefinedMenuItem::separator(app_handle)?,
        ],
    )?;
    let view_menu = Submenu::with_items(
        app_handle,
        "View",
        true,
        &[
            &zoom_in_item,
            &zoom_out_item,
            &zoom_reset_item,
            &PredefinedMenuItem::separator(app_handle)?,
        ],
    )?;

    let menu = Menu::with_items(
        app_handle,
        &[
            &Submenu::with_items(
                app_handle,
                app_name,
                true,
                &[
                    &PredefinedMenuItem::about(
                        app_handle,
                        Some("About T3.chat"),
                        Some(about_metadata),
                    )?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::services(app_handle, None)?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::hide(app_handle, None)?,
                    &PredefinedMenuItem::hide_others(app_handle, None)?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::quit(app_handle, None)?,
                ],
            )?,
            &Submenu::with_items(
                app_handle,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app_handle, None)?,
                    &PredefinedMenuItem::redo(app_handle, None)?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::cut(app_handle, None)?,
                    &PredefinedMenuItem::copy(app_handle, None)?,
                    &PredefinedMenuItem::paste(app_handle, None)?,
                    &PredefinedMenuItem::select_all(app_handle, None)?,
                ],
            )?,
            &view_menu,
            &window_menu,
        ],
    )?;

    app_handle.set_menu(menu)?;
    let zoom_level_for_menu = Arc::clone(&zoom_level);
    app_handle.on_menu_event(move |app, event| {
        if let Some(window) = app.get_webview_window("main") {
            let id = event.id().as_ref();
            match id {
                "zoom-in" | "zoom-out" | "zoom-reset" => {
                    const MIN_ZOOM: f64 = 0.5;
                    const MAX_ZOOM: f64 = 2.0;
                    const ZOOM_STEP: f64 = 0.1;

                    if let Ok(mut zoom) = zoom_level_for_menu.lock() {
                        match id {
                            "zoom-in" => {
                                *zoom = (*zoom + ZOOM_STEP).min(MAX_ZOOM);
                            }
                            "zoom-out" => {
                                *zoom = (*zoom - ZOOM_STEP).max(MIN_ZOOM);
                            }
                            "zoom-reset" => {
                                *zoom = 1.0;
                            }
                            _ => {}
                        }
                        let _ = window.set_zoom(*zoom);
                        save_cached_zoom_level(app, *zoom);
                    }
                }
                _ => {}
            }
        }
    });

    Ok(())
}

fn parse_css_rgb_component(component: &str) -> Option<u8> {
    let component = component.trim();
    if let Some(percent) = component.strip_suffix('%') {
        let value = percent.parse::<f32>().ok()?.clamp(0.0, 100.0);
        return Some(((value / 100.0) * 255.0).round() as u8);
    }

    let value = component.parse::<f32>().ok()?.clamp(0.0, 255.0);
    Some(value.round() as u8)
}

fn parse_css_alpha_component(component: &str) -> Option<u8> {
    let component = component.trim();
    if let Some(percent) = component.strip_suffix('%') {
        let value = percent.parse::<f32>().ok()?.clamp(0.0, 100.0);
        return Some(((value / 100.0) * 255.0).round() as u8);
    }

    let value = component.parse::<f32>().ok()?.clamp(0.0, 1.0);
    Some((value * 255.0).round() as u8)
}

fn parse_css_color(input: &str) -> Option<tauri::window::Color> {
    let value = input.trim();

    let components = value
        .strip_prefix("rgb(")
        .and_then(|v| v.strip_suffix(')'))
        .or_else(|| {
            value
                .strip_prefix("rgba(")
                .and_then(|v| v.strip_suffix(')'))
        })?;

    let normalized = components.replace(',', " ").replace('/', " ");
    let parts: Vec<&str> = normalized.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    let r = parse_css_rgb_component(parts[0])?;
    let g = parse_css_rgb_component(parts[1])?;
    let b = parse_css_rgb_component(parts[2])?;
    let a = parts
        .get(3)
        .and_then(|component| parse_css_alpha_component(component))
        .unwrap_or(255);

    Some((r, g, b, a).into())
}

#[cfg(target_os = "macos")]
fn apply_macos_titlebar_color<R: Runtime>(
    webview: &tauri::Webview<R>,
    color: tauri::window::Color,
) {
    if let Err(error) = webview.set_background_color(Some(color)) {
        eprintln!("failed to apply macOS webview color: {error}");
    }
    if let Err(error) = webview.window().set_background_color(Some(color)) {
        eprintln!("failed to apply macOS titlebar color: {error}");
    }
}

#[cfg(not(target_os = "macos"))]
fn apply_macos_titlebar_color<R: Runtime>(
    _webview: &tauri::Webview<R>,
    _color: tauri::window::Color,
) {
}

const BACKGROUND_COLOR_BRIDGE_SCRIPT: &str = r#"
(() => {
  if (window.__t3chatColorBridgeInstalled) return;
  window.__t3chatColorBridgeInstalled = true;
  const clamp = (value, min, max) => Math.max(min, Math.min(max, value));
  const parseRgb = (part) => {
    const value = part.trim();
    if (value.endsWith('%')) {
      const percent = clamp(parseFloat(value), 0, 100);
      if (!Number.isFinite(percent)) return null;
      return Math.round((percent / 100) * 255);
    }
    const number = clamp(parseFloat(value), 0, 255);
    if (!Number.isFinite(number)) return null;
    return Math.round(number);
  };
  const parseAlpha = (part) => {
    const value = part.trim();
    if (value.endsWith('%')) {
      const percent = clamp(parseFloat(value), 0, 100);
      if (!Number.isFinite(percent)) return null;
      return Math.round((percent / 100) * 255);
    }
    const number = clamp(parseFloat(value), 0, 1);
    if (!Number.isFinite(number)) return null;
    return Math.round(number * 255);
  };
  const parseColor = (raw) => {
    const value = (raw || '').trim();
    if (!value) return null;
    const match = value.match(/^rgba?\((.*)\)$/i);
    if (!match) return null;
    const parts = match[1].replace(/,/g, ' ').replace(/\//g, ' ').split(/\s+/).filter(Boolean);
    if (parts.length < 3) return null;
    const r = parseRgb(parts[0]);
    const g = parseRgb(parts[1]);
    const b = parseRgb(parts[2]);
    if ([r, g, b].some((n) => n == null)) return null;
    const a = parts[3] != null ? parseAlpha(parts[3]) : 255;
    if (a == null) return null;
    return [r, g, b, a];
  };
  const composite = (fg, bg) => {
    const fa = fg[3] / 255;
    const ba = bg[3] / 255;
    const outA = fa + ba * (1 - fa);
    if (outA <= 0) return [0, 0, 0, 0];
    const outR = Math.round((fg[0] * fa + bg[0] * ba * (1 - fa)) / outA);
    const outG = Math.round((fg[1] * fa + bg[1] * ba * (1 - fa)) / outA);
    const outB = Math.round((fg[2] * fa + bg[2] * ba * (1 - fa)) / outA);
    return [outR, outG, outB, Math.round(outA * 255)];
  };
  const toCssRgb = (rgba) => {
    return `rgb(${rgba[0]}, ${rgba[1]}, ${rgba[2]})`;
  };

  const report = () => {
    if (!document.body) return;

    let rawColor = getComputedStyle(document.body).backgroundColor;
    if (!rawColor || rawColor === 'transparent' || rawColor === 'rgba(0, 0, 0, 0)') {
      rawColor = getComputedStyle(document.documentElement).backgroundColor;
    }

    const body = parseColor(rawColor);
    if (!body) return;

    const htmlRaw = getComputedStyle(document.documentElement).backgroundColor;
    const html = parseColor(htmlRaw);

    let effective = body;
    if (effective[3] < 255 && html) {
      effective = composite(effective, html);
    }
    if (effective[3] < 255) {
      effective = [effective[0], effective[1], effective[2], 255];
    }

    const color = toCssRgb(effective);
    const preloadStyle = document.getElementById('t3chat-preload-bg');
    if (preloadStyle) preloadStyle.remove();

    if (!color || color === window.__t3chatLastReportedColor) return;
    window.__t3chatLastReportedColor = color;
    try {
      localStorage.setItem('t3chat.desktopBgColor', color);
    } catch {}

    const iframe = document.createElement('iframe');
    iframe.style.display = 'none';
    iframe.src = `t3chat-bg://color?value=${encodeURIComponent(color)}`;
    document.documentElement.appendChild(iframe);
    setTimeout(() => iframe.remove(), 0);
  };

  const ensureBodyObserver = () => {
    if (!document.body) {
      requestAnimationFrame(ensureBodyObserver);
      return;
    }
    const observer = new MutationObserver(report);
    observer.observe(document.documentElement, {
      attributes: true,
      childList: true,
      subtree: true
    });
  };

  ensureBodyObserver();
  window.addEventListener('load', report);
  setInterval(report, 1200);
  report();
})();
"#;

fn should_open_in_system_browser(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }

    let Some(host) = url.host_str() else {
        return false;
    };

    let is_google_auth_host = host == "accounts.google.com"
        || host.ends_with(".google.com")
        || host.ends_with(".googleusercontent.com");

    let is_workos_auth_host =
        host == "workos.com" || host.ends_with(".workos.com") || host.ends_with(".authkit.app");

    !is_t3_chat_host(host) && !is_google_auth_host && !is_workos_auth_host
}

fn external_navigation_plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("external-navigation")
        .js_init_script(BG_PRELOAD_SCRIPT)
        .on_navigation(|webview, url| {
            if url.scheme() == "t3chat-bg" {
                let color = url
                    .query_pairs()
                    .find(|(key, _)| key == "value")
                    .map(|(_, value)| value.into_owned())
                    .and_then(|value| parse_css_color(&value));

                if let Some(color) = color {
                    apply_macos_titlebar_color(webview, color);
                    save_cached_bg_color(webview, color);
                }
                return false;
            }

            if should_open_in_system_browser(url) {
                if let Err(error) = open_url(url.as_str(), None::<&str>) {
                    eprintln!("failed to open external URL {url}: {error}");
                    return true;
                }
                return false;
            }

            true
        })
        .on_page_load(|webview, payload| {
            if payload.event() != tauri::webview::PageLoadEvent::Finished {
                return;
            }

            let host = payload.url().host_str().unwrap_or_default();
            if !is_t3_chat_host(host) {
                return;
            }

            if let Err(error) = webview.eval(BACKGROUND_COLOR_BRIDGE_SCRIPT) {
                eprintln!("failed to inject background color observer: {error}");
            }
        })
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .enable_macos_default_menu(false)
        .setup(|app| {
            use tauri::Manager;

            if let Some(window) = app.get_webview_window("main") {
                let persisted_state = load_cached_window_state(&window);
                if let (Some(x), Some(y)) = (persisted_state.x, persisted_state.y) {
                    let _ = window.set_position(tauri::Position::Physical(
                        tauri::PhysicalPosition::new(x, y),
                    ));
                }
                if let (Some(width), Some(height)) = (persisted_state.width, persisted_state.height)
                {
                    let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                        width.max(1.0),
                        height.max(1.0),
                    )));
                }

                let zoom = persisted_state.zoom.unwrap_or(1.0).clamp(0.5, 2.0);
                let _ = window.set_zoom(zoom);

                let window_for_events = window.clone();
                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::Resized(_) => save_cached_window_size(&window_for_events),
                    tauri::WindowEvent::Moved(_) => save_cached_window_position(&window_for_events),
                    _ => {}
                });

                #[cfg(target_os = "macos")]
                if let Some(cached) = load_cached_bg_color(&window) {
                    let _ = window.set_background_color(Some(cached));
                }

                #[cfg(target_os = "macos")]
                {
                    let zoom_state =
                        std::sync::Arc::new(std::sync::Mutex::new(zoom.clamp(0.5, 2.0)));
                    setup_macos_app_menu(&app.handle(), zoom_state)?;
                }
            }
            Ok(())
        })
        .plugin(external_navigation_plugin())
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
