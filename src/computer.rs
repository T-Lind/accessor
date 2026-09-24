//! Desktop control through the MCP `computer` tool: screenshots plus mouse and
//! keyboard input.
//!
//! The action set mirrors the standard computer-use tools
//! (`computer_toolset_20260801`): screenshot, zoom, left/right/middle/double/
//! triple click, drag, move, mouse down/up, scroll, type, key, hold_key, wait,
//! and cursor_position.
//!
//! Off unless `computer.enabled` is set. Coordinates are always in the pixels
//! of the most recent screenshot (top-left origin). Screenshots are downscaled
//! to `computer.max-image-dimension`, and coordinates are mapped back to native
//! display pixels automatically, so callers never need to know the scale.
use crate::config::Settings;
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use enigo::{
    Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings as EnigoSettings,
};
use image::RgbaImage;
use serde_json::{json, Value};
use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};

/// Every supported member action, in the order of the standard toolset.
pub const ACTIONS: &[&str] = &[
    "screenshot",
    "zoom",
    "left_click",
    "right_click",
    "middle_click",
    "double_click",
    "triple_click",
    "left_click_drag",
    "mouse_move",
    "left_mouse_down",
    "left_mouse_up",
    "cursor_position",
    "scroll",
    "type",
    "key",
    "hold_key",
    "wait",
    "open_app",
    "list_windows",
    "focus_window",
    "ui_snapshot",
    "ui_invoke",
    "ui_set_value",
    "ui_select",
    "ui_expand",
];

pub fn enabled(settings: &Settings) -> bool {
    settings.computer.enabled
}

/// The MCP tool definition. Advertised only when computer use is enabled.
pub fn tool() -> Value {
    json!({
        "name":"computer",
        "description":"Control the real local desktop (not a browser sandbox). Prefer the reliable actions over pixel clicks: use open_app to launch an application by name, ui_snapshot to read the focused window's accessibility tree, then ui_invoke/ui_set_value to act on a named control, and ui_select/ui_expand for list items, dropdowns and menus. list_windows and focus_window switch between open windows. Fall back to screenshot/coordinate actions only for surfaces that expose no named controls or when the accessibility tree is insufficient. Screenshot and zoom return an image. Coordinates, where used, are in the pixels of the most recent screenshot (top-left origin); downscaled screenshots are mapped back to native pixels automatically. Requires computer use to be enabled in Accessor settings. This drives the real machine, so only act on explicit user requests and prefer typed confirmation for consequential actions.",
        "inputSchema":{
            "type":"object",
            "properties":{
                "action":{"type":"string","enum":ACTIONS},
                "coordinate":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2,"description":"[x, y] in screenshot pixels"},
                "start_coordinate":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2,"description":"[x, y] drag origin in screenshot pixels"},
                "region":{"type":"array","items":{"type":"integer"},"minItems":4,"maxItems":4,"description":"[x0, y0, x1, y1] in screenshot pixels"},
                "text":{"type":"string","description":"Literal text for type; a key or +-joined combo such as Return, ctrl+s for key/hold_key; modifier keys held during a click or scroll"},
                "duration":{"type":"number","description":"Seconds, 0–300"},
                "scroll_direction":{"type":"string","enum":["up","down","left","right"]},
                "scroll_amount":{"type":"integer","minimum":1,"maximum":100},
                "repeat":{"type":"integer","minimum":1,"maximum":100},
                "display":{"type":"integer","minimum":0,"description":"Display index for screenshot/zoom; defaults to the primary display"},
                "app":{"type":"string","description":"Application name for open_app, e.g. Calculator, Notepad, Chrome"},
                "window":{"type":"string","description":"Window title substring for focus_window"},
                "name":{"type":"string","description":"Control name for ui_invoke/ui_set_value when not using ref"},
                "control_type":{"type":"string","description":"Optional control role filter such as Button, Edit, ListItem, MenuItem"},
                "ref":{"type":"integer","minimum":1,"description":"Control reference from the most recent ui_snapshot"},
                "max":{"type":"integer","minimum":1,"maximum":400,"description":"Maximum controls to return from ui_snapshot"}
            },
            "required":["action"],
            "additionalProperties":false
        },
        "annotations":{"readOnlyHint":false,"destructiveHint":true,"openWorldHint":true}
    })
}

/// The coordinate space of the last returned screenshot, so input actions can
/// map screenshot pixels back to native display pixels.
#[derive(Clone, Copy)]
struct View {
    display: usize,
    origin: (i32, i32),
    native: (u32, u32),
    shown: (u32, u32),
}
static VIEW: OnceLock<Mutex<Option<View>>> = OnceLock::new();

fn set_view(view: View) {
    if let Ok(mut guard) = VIEW.get_or_init(|| Mutex::new(None)).lock() {
        *guard = Some(view);
    }
}
fn stored_view() -> Option<View> {
    VIEW.get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|guard| *guard)
}

struct Capture {
    image: RgbaImage,
    origin: (i32, i32),
    display: usize,
}

fn pick_monitor(display: Option<usize>) -> Result<(xcap::Monitor, usize)> {
    let monitors = xcap::Monitor::all().context("Cannot enumerate displays")?;
    ensure!(!monitors.is_empty(), "No displays found");
    let index = match display {
        Some(index) => index,
        None => monitors
            .iter()
            .position(|monitor| monitor.is_primary().unwrap_or(false))
            .unwrap_or(0),
    };
    ensure!(index < monitors.len(), "Display {index} does not exist");
    Ok((monitors.into_iter().nth(index).unwrap(), index))
}

fn capture(display: Option<usize>) -> Result<Capture> {
    let (monitor, index) = pick_monitor(display)?;
    let origin = (monitor.x()?, monitor.y()?);
    let image = monitor
        .capture_image()
        .context("Screen capture failed; is a desktop session active?")?;
    Ok(Capture {
        image,
        origin,
        display: index,
    })
}

fn fit_dims(native: (u32, u32), max_dim: u32) -> (u32, u32) {
    let longest = native.0.max(native.1);
    if longest <= max_dim || longest == 0 {
        return native;
    }
    let scale = max_dim as f64 / longest as f64;
    (
        ((native.0 as f64 * scale).round() as u32).max(1),
        ((native.1 as f64 * scale).round() as u32).max(1),
    )
}

fn fit(image: &RgbaImage, max_dim: u32) -> (RgbaImage, (u32, u32)) {
    let native = (image.width(), image.height());
    let shown = fit_dims(native, max_dim);
    if shown == native {
        return (image.clone(), shown);
    }
    (
        image::imageops::resize(
            image,
            shown.0,
            shown.1,
            image::imageops::FilterType::Triangle,
        ),
        shown,
    )
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image.clone())
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .context("PNG encoding failed")?;
    Ok(bytes)
}

fn text_block(text: impl Into<String>) -> Value {
    json!({"type":"text","text":text.into()})
}
fn image_block(png: &[u8]) -> Value {
    json!({"type":"image","data":STANDARD.encode(png),"mimeType":"image/png"})
}

fn current_view(settings: &Settings) -> Result<View> {
    if let Some(view) = stored_view() {
        return Ok(view);
    }
    let (monitor, display) = pick_monitor(None)?;
    let native = (monitor.width()?, monitor.height()?);
    Ok(View {
        display,
        origin: (monitor.x()?, monitor.y()?),
        native,
        shown: fit_dims(native, settings.computer.max_image_dimension),
    })
}

fn to_screen(view: &View, x: i64, y: i64) -> (i32, i32) {
    let sx = view.native.0 as f64 / view.shown.0 as f64;
    let sy = view.native.1 as f64 / view.shown.1 as f64;
    (
        view.origin.0 + (x as f64 * sx).round() as i32,
        view.origin.1 + (y as f64 * sy).round() as i32,
    )
}

fn to_image(view: &View, x: i32, y: i32) -> (i32, i32) {
    let sx = view.shown.0 as f64 / view.native.0 as f64;
    let sy = view.shown.1 as f64 / view.native.1 as f64;
    (
        ((x - view.origin.0) as f64 * sx).round() as i32,
        ((y - view.origin.1) as f64 * sy).round() as i32,
    )
}

fn coord(args: &Value, key: &str) -> Result<Option<(i64, i64)>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let pair = value.as_array().context("coordinate must be [x, y]")?;
            ensure!(pair.len() == 2, "coordinate must be [x, y]");
            Ok(Some((
                pair[0].as_i64().context("x must be an integer")?,
                pair[1].as_i64().context("y must be an integer")?,
            )))
        }
    }
}

fn region(args: &Value) -> Result<(i64, i64, i64, i64)> {
    let boxed = args["region"]
        .as_array()
        .context("zoom needs region [x0, y0, x1, y1]")?;
    ensure!(boxed.len() == 4, "region must be [x0, y0, x1, y1]");
    let mut values = [0_i64; 4];
    for (slot, value) in values.iter_mut().zip(boxed) {
        *slot = value.as_i64().context("region values must be integers")?;
    }
    Ok((values[0], values[1], values[2], values[3]))
}

fn text_arg(args: &Value) -> Option<&str> {
    args.get("text").and_then(Value::as_str)
}

fn duration_arg(args: &Value) -> Result<Duration> {
    let secs = args
        .get("duration")
        .context("duration is required")?
        .as_f64()
        .context("duration must be a number of seconds")?;
    ensure!(
        secs.is_finite() && (0.0..=300.0).contains(&secs),
        "duration must be 0–300 seconds"
    );
    Ok(Duration::from_secs_f64(secs))
}

fn input() -> Result<Enigo> {
    Enigo::new(&EnigoSettings::default())
        .context("Cannot open the desktop input session; a local desktop session must be active")
}

pub(crate) fn input_handle() -> Result<Enigo> {
    input()
}

pub(crate) fn click_point(x: i32, y: i32) -> Result<()> {
    let mut enigo = input()?;
    enigo.move_mouse(x, y, Coordinate::Abs)?;
    enigo.button(Button::Left, Direction::Click)?;
    Ok(())
}

pub(crate) fn type_at_focus(text: &str) -> Result<()> {
    input()?.text(text)?;
    Ok(())
}

pub(crate) fn key_combo(enigo: &mut Enigo, combo: &str, repeat: u32) -> Result<()> {
    let keys = parse_combo(combo)?;
    for _ in 0..repeat.clamp(1, 100) {
        hold(enigo, &keys)?;
        release(enigo, &keys)?;
    }
    Ok(())
}

fn parse_key(token: &str) -> Result<Key> {
    let token = token.trim();
    ensure!(!token.is_empty(), "Empty key");
    let lower = token.to_ascii_lowercase();
    Ok(match lower.as_str() {
        "return" | "enter" => Key::Return,
        "tab" => Key::Tab,
        "escape" | "esc" => Key::Escape,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "page_up" | "pgup" => Key::PageUp,
        "pagedown" | "page_down" | "pgdn" => Key::PageDown,
        "up" | "uparrow" | "arrowup" => Key::UpArrow,
        "down" | "downarrow" | "arrowdown" => Key::DownArrow,
        "left" | "leftarrow" | "arrowleft" => Key::LeftArrow,
        "right" | "rightarrow" | "arrowright" => Key::RightArrow,
        "ctrl" | "control" => Key::Control,
        "alt" | "option" => Key::Alt,
        "shift" => Key::Shift,
        "meta" | "cmd" | "command" | "super" | "win" | "windows" => Key::Meta,
        "capslock" | "caps" => Key::CapsLock,
        "printscreen" | "printscr" | "prtsc" => Key::PrintScr,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        _ if token.chars().count() == 1 => Key::Unicode(token.chars().next().unwrap()),
        _ => bail!("Unknown key: {token}"),
    })
}

fn parse_combo(text: &str) -> Result<Vec<Key>> {
    let keys = text
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(parse_key)
        .collect::<Result<Vec<_>>>()?;
    ensure!(!keys.is_empty(), "No key given");
    Ok(keys)
}

fn modifiers(args: &Value) -> Result<Vec<Key>> {
    match text_arg(args) {
        Some(text) if !text.trim().is_empty() => parse_combo(text),
        _ => Ok(Vec::new()),
    }
}

fn hold(enigo: &mut Enigo, keys: &[Key]) -> Result<()> {
    for key in keys {
        enigo.key(*key, Direction::Press)?;
    }
    Ok(())
}
fn release(enigo: &mut Enigo, keys: &[Key]) -> Result<()> {
    for key in keys.iter().rev() {
        enigo.key(*key, Direction::Release)?;
    }
    Ok(())
}

fn screenshot(settings: &Settings, args: &Value) -> Result<Vec<Value>> {
    let display = args
        .get("display")
        .and_then(Value::as_i64)
        .map(|value| value.max(0) as usize);
    let capture = capture(display)?;
    let native = (capture.image.width(), capture.image.height());
    let (shown_image, shown) = fit(&capture.image, settings.computer.max_image_dimension);
    let png = encode_png(&shown_image)?;
    set_view(View {
        display: capture.display,
        origin: capture.origin,
        native,
        shown,
    });
    let text = format!(
        "Screenshot of display {} ({}x{} native, shown {}x{}). Coordinates are in this image's pixels, origin top-left.",
        capture.display, native.0, native.1, shown.0, shown.1
    );
    Ok(vec![text_block(text), image_block(&png)])
}

fn zoom(settings: &Settings, args: &Value) -> Result<Vec<Value>> {
    let (x0, y0, x1, y1) = region(args)?;
    ensure!(
        x1 > x0 && y1 > y0,
        "region must be [x0, y0, x1, y1] with x1 > x0 and y1 > y0"
    );
    let view = current_view(settings)?;
    let capture = capture(Some(view.display))?;
    let sx = view.native.0 as f64 / view.shown.0 as f64;
    let sy = view.native.1 as f64 / view.shown.1 as f64;
    let nx0 = (x0 as f64 * sx).floor().max(0.0) as u32;
    let ny0 = (y0 as f64 * sy).floor().max(0.0) as u32;
    let nx1 = ((x1 as f64 * sx).ceil() as u32).min(capture.image.width());
    let ny1 = ((y1 as f64 * sy).ceil() as u32).min(capture.image.height());
    ensure!(nx1 > nx0 && ny1 > ny0, "Region is outside the display");
    let cropped =
        image::imageops::crop_imm(&capture.image, nx0, ny0, nx1 - nx0, ny1 - ny0).to_image();
    let (shown_image, shown) = fit(&cropped, settings.computer.zoom_dimension);
    let png = encode_png(&shown_image)?;
    let text = format!(
        "Zoom of region [{x0}, {y0}, {x1}, {y1}] shown at {}x{}. Continue to use full-screenshot coordinates.",
        shown.0, shown.1
    );
    Ok(vec![text_block(text), image_block(&png)])
}

fn mouse_move(settings: &Settings, args: &Value) -> Result<Vec<Value>> {
    let (x, y) = coord(args, "coordinate")?.context("mouse_move needs coordinate [x, y]")?;
    let view = current_view(settings)?;
    let (screen_x, screen_y) = to_screen(&view, x, y);
    input()?.move_mouse(screen_x, screen_y, Coordinate::Abs)?;
    Ok(vec![text_block(format!("Moved cursor to {x}, {y}"))])
}

fn mouse_button(
    settings: &Settings,
    args: &Value,
    button: Button,
    direction: Direction,
    label: &str,
) -> Result<Vec<Value>> {
    let view = current_view(settings)?;
    let mut enigo = input()?;
    if let Some((x, y)) = coord(args, "coordinate")? {
        let (screen_x, screen_y) = to_screen(&view, x, y);
        enigo.move_mouse(screen_x, screen_y, Coordinate::Abs)?;
    }
    enigo.button(button, direction)?;
    Ok(vec![text_block(label)])
}

fn click(
    settings: &Settings,
    args: &Value,
    button: Button,
    count: u32,
    label: &str,
) -> Result<Vec<Value>> {
    let view = current_view(settings)?;
    let mut enigo = input()?;
    if let Some((x, y)) = coord(args, "coordinate")? {
        let (screen_x, screen_y) = to_screen(&view, x, y);
        enigo.move_mouse(screen_x, screen_y, Coordinate::Abs)?;
    }
    let held = modifiers(args)?;
    hold(&mut enigo, &held)?;
    let mut outcome: Result<()> = Ok(());
    for _ in 0..count {
        if let Err(error) = enigo.button(button, Direction::Click) {
            outcome = Err(error.into());
            break;
        }
    }
    let released = release(&mut enigo, &held);
    outcome?;
    released?;
    Ok(vec![text_block(label)])
}

fn drag(settings: &Settings, args: &Value) -> Result<Vec<Value>> {
    let start = coord(args, "start_coordinate")?
        .context("left_click_drag needs start_coordinate [x, y]")?;
    let end = coord(args, "coordinate")?.context("left_click_drag needs coordinate [x, y]")?;
    let view = current_view(settings)?;
    let mut enigo = input()?;
    let (start_x, start_y) = to_screen(&view, start.0, start.1);
    let (end_x, end_y) = to_screen(&view, end.0, end.1);
    let held = modifiers(args)?;
    enigo.move_mouse(start_x, start_y, Coordinate::Abs)?;
    hold(&mut enigo, &held)?;
    let mut outcome: Result<()> = Ok(());
    for step in [
        enigo.button(Button::Left, Direction::Press),
        enigo.move_mouse(end_x, end_y, Coordinate::Abs),
        enigo.button(Button::Left, Direction::Release),
    ] {
        if let Err(error) = step {
            outcome = Err(error.into());
            break;
        }
    }
    let released = release(&mut enigo, &held);
    outcome?;
    released?;
    Ok(vec![text_block("Dragged")])
}

fn cursor_position(settings: &Settings) -> Result<Vec<Value>> {
    let view = current_view(settings)?;
    let (x, y) = input()?.location()?;
    let (image_x, image_y) = to_image(&view, x, y);
    Ok(vec![text_block(format!("X={image_x}, Y={image_y}"))])
}

fn scroll(settings: &Settings, args: &Value) -> Result<Vec<Value>> {
    let direction = args["scroll_direction"]
        .as_str()
        .context("scroll needs scroll_direction")?;
    let amount = args["scroll_amount"].as_i64().unwrap_or(1).clamp(1, 100) as i32;
    let (length, axis) = match direction {
        "up" => (-amount, Axis::Vertical),
        "down" => (amount, Axis::Vertical),
        "left" => (-amount, Axis::Horizontal),
        "right" => (amount, Axis::Horizontal),
        _ => bail!("scroll_direction must be up, down, left, or right"),
    };
    let view = current_view(settings)?;
    let mut enigo = input()?;
    if let Some((x, y)) = coord(args, "coordinate")? {
        let (screen_x, screen_y) = to_screen(&view, x, y);
        enigo.move_mouse(screen_x, screen_y, Coordinate::Abs)?;
    }
    let held = modifiers(args)?;
    hold(&mut enigo, &held)?;
    let scrolled = enigo.scroll(length, axis);
    let released = release(&mut enigo, &held);
    scrolled?;
    released?;
    Ok(vec![text_block(format!("Scrolled {direction} {amount}"))])
}

fn type_text(args: &Value) -> Result<Vec<Value>> {
    let text = text_arg(args).context("type needs text")?;
    let count = text.chars().count();
    ensure!(count <= 10_000, "text is too long (limit 10000 characters)");
    input()?.text(text)?;
    Ok(vec![text_block(format!("Typed {count} characters"))])
}

fn key_action(args: &Value) -> Result<Vec<Value>> {
    let text = text_arg(args).context("key needs text")?;
    let keys = parse_combo(text)?;
    let repeat = args["repeat"].as_i64().unwrap_or(1).clamp(1, 100);
    let mut enigo = input()?;
    for _ in 0..repeat {
        hold(&mut enigo, &keys)?;
        release(&mut enigo, &keys)?;
    }
    Ok(vec![text_block(format!("Pressed {text}"))])
}

fn hold_key(args: &Value) -> Result<Vec<Value>> {
    let text = text_arg(args).context("hold_key needs text")?;
    let keys = parse_combo(text)?;
    let duration = duration_arg(args)?;
    let mut enigo = input()?;
    hold(&mut enigo, &keys)?;
    std::thread::sleep(duration);
    release(&mut enigo, &keys)?;
    Ok(vec![text_block(format!(
        "Held {text} for {} s",
        duration.as_secs_f64()
    ))])
}

fn dispatch(action: &str, args: &Value, settings: &Settings) -> Result<Vec<Value>> {
    match action {
        "screenshot" => screenshot(settings, args),
        "zoom" => zoom(settings, args),
        "mouse_move" => mouse_move(settings, args),
        "left_click" => click(settings, args, Button::Left, 1, "Left-clicked"),
        "right_click" => click(settings, args, Button::Right, 1, "Right-clicked"),
        "middle_click" => click(settings, args, Button::Middle, 1, "Middle-clicked"),
        "double_click" => click(settings, args, Button::Left, 2, "Double-clicked"),
        "triple_click" => click(settings, args, Button::Left, 3, "Triple-clicked"),
        "left_mouse_down" => mouse_button(
            settings,
            args,
            Button::Left,
            Direction::Press,
            "Left button pressed",
        ),
        "left_mouse_up" => mouse_button(
            settings,
            args,
            Button::Left,
            Direction::Release,
            "Left button released",
        ),
        "left_click_drag" => drag(settings, args),
        "cursor_position" => cursor_position(settings),
        "scroll" => scroll(settings, args),
        "type" => type_text(args),
        "key" => key_action(args),
        "hold_key" => hold_key(args),
        "open_app" => {
            let app = args
                .get("app")
                .and_then(Value::as_str)
                .or_else(|| text_arg(args))
                .context("open_app needs an app name")?;
            Ok(vec![text_block(crate::desktop::open_app(app)?)])
        }
        "list_windows" => Ok(vec![text_block(crate::desktop::list_windows()?)]),
        "focus_window" => {
            let window = args
                .get("window")
                .and_then(Value::as_str)
                .or_else(|| text_arg(args))
                .context("focus_window needs a window title")?;
            Ok(vec![text_block(crate::desktop::focus_window(window)?)])
        }
        "ui_snapshot" => {
            let max = args["max"].as_u64().unwrap_or(200) as usize;
            Ok(vec![text_block(crate::desktop::ui_snapshot(max)?)])
        }
        "ui_invoke" => {
            let reference = args["ref"].as_u64().map(|value| value as usize);
            let name = args["name"].as_str().unwrap_or("");
            let control_type = args["control_type"].as_str().unwrap_or("");
            Ok(vec![text_block(crate::desktop::ui_invoke(
                reference,
                name,
                control_type,
            )?)])
        }
        "ui_set_value" => {
            let reference = args["ref"].as_u64().map(|value| value as usize);
            let name = args["name"].as_str().unwrap_or("");
            let control_type = args["control_type"].as_str().unwrap_or("");
            let text = text_arg(args).context("ui_set_value needs text")?;
            Ok(vec![text_block(crate::desktop::ui_set_value(
                reference,
                name,
                control_type,
                text,
            )?)])
        }
        "ui_select" => {
            let reference = args["ref"].as_u64().map(|value| value as usize);
            let name = args["name"].as_str().unwrap_or("");
            let control_type = args["control_type"].as_str().unwrap_or("");
            Ok(vec![text_block(crate::desktop::ui_select(
                reference,
                name,
                control_type,
            )?)])
        }
        "ui_expand" => {
            let reference = args["ref"].as_u64().map(|value| value as usize);
            let name = args["name"].as_str().unwrap_or("");
            let control_type = args["control_type"].as_str().unwrap_or("");
            Ok(vec![text_block(crate::desktop::ui_expand(
                reference,
                name,
                control_type,
            )?)])
        }
        _ => bail!("Unknown computer action: {action}"),
    }
}

/// Run one `computer` action and return MCP content blocks.
pub async fn call(args: &Value, settings: &Settings) -> Result<Vec<Value>> {
    let action = args["action"].as_str().context("action is required")?;
    ensure!(
        ACTIONS.contains(&action),
        "Unknown computer action: {action}"
    );
    if action == "wait" {
        let duration = duration_arg(args)?;
        tokio::time::sleep(duration).await;
        return Ok(vec![text_block(format!(
            "Waited {} s",
            duration.as_secs_f64()
        ))]);
    }
    let action = action.to_string();
    let args = args.clone();
    let settings = settings.clone();
    tokio::task::spawn_blocking(move || dispatch(&action, &args, &settings))
        .await
        .context("Computer action task failed")?
}

/// Human-readable readiness report used by `acc doctor` and `acc computer`.
pub fn status(settings: &Settings) -> String {
    let mut lines = vec![format!(
        "Computer use: {}",
        if settings.computer.enabled {
            "enabled".to_string()
        } else {
            "disabled — enable with `acc computer enable`".to_string()
        }
    )];
    match xcap::Monitor::all() {
        Ok(monitors) => {
            lines.push(format!("Displays: {}", monitors.len()));
            for (index, monitor) in monitors.iter().enumerate() {
                let primary = if monitor.is_primary().unwrap_or(false) {
                    " (primary)"
                } else {
                    ""
                };
                lines.push(format!(
                    "  display {index}{primary}: {}x{} at {},{}",
                    monitor.width().unwrap_or(0),
                    monitor.height().unwrap_or(0),
                    monitor.x().unwrap_or(0),
                    monitor.y().unwrap_or(0)
                ));
            }
        }
        Err(error) => lines.push(format!("Displays: unavailable ({error})")),
    }
    lines.push(
        "Input: enigo — Windows, macOS, Linux X11. Wayland needs a libei/ydotool bridge."
            .to_string(),
    );
    lines.push(crate::desktop::capabilities());
    lines.join("\n")
}

/// Capture a display and optionally save it, without using the MCP surface.
pub fn test(
    settings: &Settings,
    display: Option<usize>,
    output: Option<&std::path::Path>,
) -> Result<String> {
    let capture = capture(display)?;
    let native = (capture.image.width(), capture.image.height());
    let mut report = format!(
        "Captured display {} at {}x{}",
        capture.display, native.0, native.1
    );
    if let Some(path) = output {
        capture
            .image
            .save(path)
            .with_context(|| format!("Cannot write {}", path.display()))?;
        report.push_str(&format!("\nSaved PNG to {}", path.display()));
    }
    report.push_str(&format!(
        "\nComputer use is {}",
        if settings.computer.enabled {
            "enabled"
        } else {
            "disabled — enable with `acc computer enable`"
        }
    ));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_action_set_is_complete() {
        for action in [
            "screenshot",
            "zoom",
            "left_click",
            "right_click",
            "middle_click",
            "double_click",
            "triple_click",
            "left_click_drag",
            "mouse_move",
            "left_mouse_down",
            "left_mouse_up",
            "cursor_position",
            "scroll",
            "type",
            "key",
            "hold_key",
            "wait",
            "open_app",
            "list_windows",
            "focus_window",
            "ui_snapshot",
            "ui_invoke",
            "ui_set_value",
            "ui_select",
            "ui_expand",
        ] {
            assert!(ACTIONS.contains(&action), "missing {action}");
        }
        assert_eq!(ACTIONS.len(), 25);
    }

    #[test]
    fn key_names_and_combos_parse() {
        assert_eq!(parse_key("Return").unwrap(), Key::Return);
        assert_eq!(parse_key("enter").unwrap(), Key::Return);
        assert_eq!(parse_key("ctrl").unwrap(), Key::Control);
        assert_eq!(parse_key("A").unwrap(), Key::Unicode('A'));
        assert_eq!(
            parse_combo("ctrl+shift+s").unwrap(),
            vec![Key::Control, Key::Shift, Key::Unicode('s')]
        );
        assert!(parse_key("nope").is_err());
    }

    #[test]
    fn coordinates_map_between_image_and_screen() {
        let view = View {
            display: 0,
            origin: (100, 50),
            native: (3840, 2160),
            shown: (1280, 720),
        };
        assert_eq!(to_screen(&view, 0, 0), (100, 50));
        assert_eq!(to_screen(&view, 1280, 720), (3940, 2210));
        assert_eq!(to_screen(&view, 640, 360), (2020, 1130));
        assert_eq!(to_image(&view, 2020, 1130), (640, 360));
    }

    #[test]
    fn fit_dims_preserves_aspect_and_caps() {
        assert_eq!(fit_dims((1920, 1080), 1280), (1280, 720));
        assert_eq!(fit_dims((1000, 800), 1280), (1000, 800));
    }

    #[test]
    fn disabled_by_default() {
        assert!(!enabled(&Settings::default()));
    }
}
