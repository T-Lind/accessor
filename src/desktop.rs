//! Reliable desktop actions that avoid pixel guessing: launching applications
//! deterministically and, on Windows, driving apps through the UI Automation
//! accessibility tree (find controls by name, invoke via control patterns).
//!
//! The accessibility tree is the current standard for reliable computer use:
//! it is independent of DPI, theme, window position and occlusion. Screenshot
//! coordinates remain the fallback for surfaces that expose no controls.
use anyhow::Result;

#[cfg(windows)]
mod platform {
    use super::*;
    use anyhow::{bail, ensure, Context};
    use std::sync::{Mutex, OnceLock};
    use uiautomation::core::{UIAutomation, UIElement, UITreeWalker};
    use uiautomation::patterns::{UIInvokePattern, UIValuePattern};

    const MAX_DEPTH: usize = 10;

    /// Interactive roles worth naming in a snapshot. Kept broad so the model
    /// can act on lists, fields and menus, not just buttons.
    const INTERACTIVE: &[&str] = &[
        "Button",
        "Hyperlink",
        "MenuItem",
        "ListItem",
        "Edit",
        "CheckBox",
        "RadioButton",
        "ComboBox",
        "TabItem",
        "TreeItem",
        "SplitButton",
        "DataItem",
        "Slider",
        "Spinner",
    ];

    #[derive(Clone)]
    struct RefTarget {
        name: String,
        control_type: String,
        pid: u32,
    }

    fn refs() -> &'static Mutex<Vec<RefTarget>> {
        static REFS: OnceLock<Mutex<Vec<RefTarget>>> = OnceLock::new();
        REFS.get_or_init(|| Mutex::new(Vec::new()))
    }

    struct Node {
        name: String,
        control_type: String,
        interactive: bool,
        point: Option<(i32, i32)>,
        pid: u32,
    }

    fn automation() -> Result<UIAutomation> {
        UIAutomation::new().context("UI Automation is unavailable on this session")
    }

    /// Curated targets for common apps. URI and executable launches are exact,
    /// so "calculator" can never be mistaken for another icon.
    fn known_app(name: &str) -> Option<&'static str> {
        Some(match name {
            "calculator" | "calc" => "calculator:",
            "notepad" => "notepad.exe",
            "settings" | "windows settings" => "ms-settings:",
            "paint" => "mspaint.exe",
            "explorer" | "files" | "file explorer" => "explorer.exe",
            "cmd" | "command prompt" | "terminal" | "powershell" => "wt.exe",
            "task manager" => "taskmgr.exe",
            "control panel" | "control" => "control.exe",
            "snipping tool" | "screenshot" | "screenclip" => "ms-screenclip:",
            "store" | "microsoft store" => "ms-windows-store:",
            "photos" => "ms-photos:",
            "camera" => "microsoft.windows.camera:",
            "word" => "winword.exe",
            "excel" => "excel.exe",
            "powerpoint" => "powerpnt.exe",
            "outlook" => "outlook.exe",
            "chrome" | "google chrome" => "chrome.exe",
            "edge" | "microsoft edge" => "msedge.exe",
            "firefox" => "firefox.exe",
            "spotify" => "spotify:",
            "clock" | "alarms" => "ms-clock:",
            "mail" => "outlookcal:",
            _ => return None,
        })
    }

    fn start(target: &str) -> Result<()> {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(target)
            .spawn()
            .with_context(|| format!("Could not launch {target}"))?;
        Ok(())
    }

    pub fn open_app(name: &str) -> Result<String> {
        let name = name.trim();
        ensure!(!name.is_empty(), "open_app needs an app name");
        let key = name.to_lowercase();
        if let Some(target) = known_app(&key) {
            start(target)?;
            return Ok(format!("Launched {name} ({target})."));
        }
        // Exact executable on PATH.
        if let Ok(output) = std::process::Command::new("where").arg(name).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !path.is_empty() {
                    start(&path)?;
                    return Ok(format!("Launched {path}."));
                }
            }
        }
        // Last resort: use the Start menu search exactly like a person would.
        super::start_search(name)?;
        Ok(format!(
            "Typed {name} into the Start search and pressed Enter. If the wrong app opens, use open_app with an exact name."
        ))
    }

    fn control_name(element: &UIElement) -> String {
        element
            .get_control_type()
            .map(|control| format!("{control:?}"))
            .unwrap_or_default()
    }

    /// Ascend from the focused element to its top-level window, so snapshots
    /// and lookups cover the whole window rather than one focused control.
    fn top_window(walker: &UITreeWalker, start: &UIElement) -> UIElement {
        let mut current = start.clone();
        let mut best = start.clone();
        for _ in 0..24 {
            match walker.get_parent(&current) {
                Ok(parent) => {
                    if control_name(&parent).eq_ignore_ascii_case("window") {
                        return parent;
                    }
                    best = parent.clone();
                    current = parent;
                }
                Err(_) => break,
            }
        }
        best
    }

    fn collect(
        walker: &UITreeWalker,
        root: &UIElement,
        depth: usize,
        max: usize,
        out: &mut Vec<Node>,
    ) {
        if depth > MAX_DEPTH || out.len() >= max {
            return;
        }
        let mut child = walker.get_first_child(root);
        while let Ok(element) = child {
            if out.len() >= max {
                break;
            }
            let name = element.get_name().unwrap_or_default();
            let control_type = control_name(&element);
            let pid = element.get_process_id().unwrap_or(0);
            let offscreen = element.is_offscreen().unwrap_or(true);
            if !name.is_empty() && !offscreen {
                let focused = element.has_keyboard_focus().unwrap_or(false);
                out.push(Node {
                    name,
                    control_type,
                    interactive: focused,
                    point: element
                        .get_clickable_point()
                        .ok()
                        .flatten()
                        .map(|point| (point.get_x(), point.get_y())),
                    pid,
                });
            }
            collect(walker, &element, depth + 1, max, out);
            child = walker.get_next_sibling(&element);
        }
    }

    fn window_root(automation: &UIAutomation) -> Result<UIElement> {
        let walker = automation.get_control_view_walker()?;
        if let Ok(focused) = automation.get_focused_element() {
            return Ok(top_window(&walker, &focused));
        }
        automation.get_root_element().context("No desktop element")
    }

    pub fn list_windows() -> Result<String> {
        let automation = automation()?;
        let root = automation.get_root_element()?;
        let walker = automation.get_control_view_walker()?;
        let mut nodes = Vec::new();
        collect(&walker, &root, 0, 200, &mut nodes);
        if nodes.is_empty() {
            return Ok("No top-level windows found.".into());
        }
        let mut seen = std::collections::HashSet::new();
        let mut lines = Vec::new();
        for node in nodes {
            if !node.control_type.eq_ignore_ascii_case("window") {
                continue;
            }
            if seen.insert(node.name.clone()) {
                lines.push(format!("{}  (pid {})", node.name, node.pid));
            }
        }
        if lines.is_empty() {
            return Ok("No top-level windows found.".into());
        }
        Ok(lines.join("\n"))
    }

    fn find(
        walker: &UITreeWalker,
        root: &UIElement,
        name: &str,
        control_type: &str,
        pid: Option<u32>,
        depth: usize,
    ) -> Option<UIElement> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut child = walker.get_first_child(root);
        while let Ok(element) = child {
            let element_name = element.get_name().unwrap_or_default();
            let element_type = control_name(&element);
            let type_ok = control_type.is_empty() || element_type == control_type;
            let pid_ok = pid.is_none_or(|value| element.get_process_id().unwrap_or(0) == value);
            if element_name.eq_ignore_ascii_case(name) && type_ok && pid_ok {
                return Some(element);
            }
            if let Some(found) = find(walker, &element, name, control_type, pid, depth + 1) {
                return Some(found);
            }
            child = walker.get_next_sibling(&element);
        }
        None
    }

    pub fn focus_window(title: &str) -> Result<String> {
        let title = title.trim();
        ensure!(!title.is_empty(), "focus_window needs a window title");
        let automation = automation()?;
        let root = automation.get_root_element()?;
        let walker = automation.get_control_view_walker()?;
        let needle = title.to_lowercase();
        let mut child = walker.get_first_child(&root);
        while let Ok(element) = child {
            let name = element.get_name().unwrap_or_default();
            if name.to_lowercase().contains(&needle) {
                element
                    .set_focus()
                    .with_context(|| format!("Could not focus {name}"))?;
                return Ok(format!("Focused {name}."));
            }
            child = walker.get_next_sibling(&element);
        }
        bail!("No open window matches {title}")
    }

    pub fn ui_snapshot(max: usize) -> Result<String> {
        let max = max.clamp(1, 400);
        let automation = automation()?;
        let root = window_root(&automation)?;
        let title = root.get_name().unwrap_or_default();
        let walker = automation.get_control_view_walker()?;
        let mut nodes = Vec::new();
        collect(&walker, &root, 0, max, &mut nodes);
        let mut targets = Vec::new();
        let mut lines = Vec::new();
        let mut index = 0usize;
        for node in &nodes {
            let interactive = node.interactive || INTERACTIVE.contains(&node.control_type.as_str());
            if !interactive {
                continue;
            }
            index += 1;
            let point = node
                .point
                .map(|(x, y)| format!(" at {x},{y}"))
                .unwrap_or_default();
            lines.push(format!(
                "[{index}] {} \"{}\"{}",
                node.control_type, node.name, point
            ));
            targets.push(RefTarget {
                name: node.name.clone(),
                control_type: node.control_type.clone(),
                pid: node.pid,
            });
        }
        if let Ok(mut guard) = refs().lock() {
            *guard = targets;
        }
        if lines.is_empty() {
            return Ok(format!(
                "Window \"{title}\" exposes no named controls. Use screenshot-based actions instead."
            ));
        }
        Ok(format!(
            "Window \"{title}\" — {} controls (refs are valid only for the most recent ui_snapshot):\n{}",
            lines.len(),
            lines.join("\n")
        ))
    }

    fn resolve(reference: Option<usize>, name: &str) -> Result<RefTarget> {
        if let Some(index) = reference.filter(|value| *value > 0) {
            let guard = refs()
                .lock()
                .map_err(|_| anyhow::anyhow!("Snapshot lock poisoned"))?;
            return guard
                .get(index - 1)
                .cloned()
                .context("Unknown ref; take a fresh ui_snapshot first");
        }
        ensure!(!name.trim().is_empty(), "Provide a ref or a control name");
        Ok(RefTarget {
            name: name.trim().to_string(),
            control_type: String::new(),
            pid: 0,
        })
    }

    fn locate(target: &RefTarget) -> Result<UIElement> {
        let automation = automation()?;
        let root = window_root(&automation)?;
        let walker = automation.get_control_view_walker()?;
        let pid = (target.pid != 0).then_some(target.pid);
        find(&walker, &root, &target.name, &target.control_type, pid, 0)
            .with_context(|| format!("Control \"{}\" is not on screen", target.name))
    }

    pub fn ui_invoke(reference: Option<usize>, name: &str, control_type: &str) -> Result<String> {
        let mut target = resolve(reference, name)?;
        if !control_type.trim().is_empty() {
            target.control_type = control_type.trim().to_string();
        }
        let element = locate(&target)?;
        // Prefer the accessibility pattern: it works without focus or occlusion.
        if let Ok(pattern) = element.get_pattern::<UIInvokePattern>() {
            pattern
                .invoke()
                .with_context(|| format!("Invoke failed for \"{}\"", target.name))?;
            return Ok(format!("Invoked \"{}\".", target.name));
        }
        if let Some(point) = element.get_clickable_point().ok().flatten() {
            crate::computer::click_point(point.get_x(), point.get_y())?;
            return Ok(format!("Clicked \"{}\".", target.name));
        }
        let rect = element.get_bounding_rectangle()?;
        let x = (rect.get_left() + rect.get_right()) / 2;
        let y = (rect.get_top() + rect.get_bottom()) / 2;
        crate::computer::click_point(x, y)?;
        Ok(format!("Clicked \"{}\" at its center.", target.name))
    }

    pub fn ui_set_value(
        reference: Option<usize>,
        name: &str,
        control_type: &str,
        text: &str,
    ) -> Result<String> {
        let mut target = resolve(reference, name)?;
        if !control_type.trim().is_empty() {
            target.control_type = control_type.trim().to_string();
        }
        let element = locate(&target)?;
        if let Ok(pattern) = element.get_pattern::<UIValuePattern>() {
            pattern
                .set_value(text)
                .with_context(|| format!("Could not set value on \"{}\"", target.name))?;
            return Ok(format!("Set \"{}\".", target.name));
        }
        element
            .set_focus()
            .with_context(|| format!("Could not focus \"{}\"", target.name))?;
        crate::computer::click_point(
            element
                .get_clickable_point()
                .ok()
                .flatten()
                .map(|point| point.get_x())
                .unwrap_or(0),
            element
                .get_clickable_point()
                .ok()
                .flatten()
                .map(|point| point.get_y())
                .unwrap_or(0),
        )
        .ok();
        crate::computer::type_at_focus(text)?;
        Ok(format!("Typed into \"{}\".", target.name))
    }

    pub fn ui_select(reference: Option<usize>, name: &str, control_type: &str) -> Result<String> {
        use uiautomation::patterns::UISelectionItemPattern;
        let mut target = resolve(reference, name)?;
        if !control_type.trim().is_empty() {
            target.control_type = control_type.trim().to_string();
        }
        let element = locate(&target)?;
        if let Ok(pattern) = element.get_pattern::<UISelectionItemPattern>() {
            pattern
                .select()
                .with_context(|| format!("Could not select \"{}\"", target.name))?;
            return Ok(format!("Selected \"{}\".", target.name));
        }
        ui_invoke(reference, name, control_type)
    }

    pub fn ui_expand(reference: Option<usize>, name: &str, control_type: &str) -> Result<String> {
        use uiautomation::patterns::UIExpandCollapsePattern;
        let mut target = resolve(reference, name)?;
        if !control_type.trim().is_empty() {
            target.control_type = control_type.trim().to_string();
        }
        let element = locate(&target)?;
        if let Ok(pattern) = element.get_pattern::<UIExpandCollapsePattern>() {
            pattern
                .expand()
                .with_context(|| format!("Could not expand \"{}\"", target.name))?;
            return Ok(format!("Expanded \"{}\".", target.name));
        }
        ui_invoke(reference, name, control_type)
    }

    pub fn capabilities() -> String {
        "Semantic UI: Windows UI Automation (snapshot, invoke, set, select, expand).".into()
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use anyhow::{ensure, Context};

    const SCRIPT: &str = include_str!("platform/macos_ax.applescript");

    fn run(args: &[&str]) -> Result<String> {
        let path = std::env::temp_dir().join("accessor-ax.applescript");
        std::fs::write(&path, SCRIPT).context("Could not write the macOS bridge script")?;
        let output = std::process::Command::new("osascript")
            .arg(&path)
            .args(args)
            .output()
            .context("Could not run osascript; is this a desktop session?")?;
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("macOS accessibility failed: {}", error.trim());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn open_app(name: &str) -> Result<String> {
        let name = name.trim();
        ensure!(!name.is_empty(), "open_app needs an app name");
        let status = std::process::Command::new("open")
            .arg("-a")
            .arg(name)
            .status()
            .with_context(|| format!("Could not open {name}"))?;
        ensure!(status.success(), "macOS could not open {name}");
        Ok(format!("Asked macOS to open {name}."))
    }

    pub fn list_windows() -> Result<String> {
        run(&["list"])
    }
    pub fn focus_window(title: &str) -> Result<String> {
        run(&["focus", title])
    }
    pub fn ui_snapshot(max: usize) -> Result<String> {
        run(&["snapshot", "", &max.to_string()])
    }
    pub fn ui_invoke(_reference: Option<usize>, name: &str, control_type: &str) -> Result<String> {
        run(&["invoke", name, control_type])
    }
    pub fn ui_select(_reference: Option<usize>, name: &str, control_type: &str) -> Result<String> {
        run(&["select", name, control_type])
    }
    pub fn ui_expand(_reference: Option<usize>, name: &str, control_type: &str) -> Result<String> {
        run(&["expand", name, control_type])
    }
    pub fn ui_set_value(
        _reference: Option<usize>,
        name: &str,
        _control_type: &str,
        text: &str,
    ) -> Result<String> {
        run(&["set", name, text])
    }
    pub fn capabilities() -> String {
        "Semantic UI: macOS Accessibility via System Events. Grant Accessibility permission to the terminal/acc; use open_app to launch apps.".into()
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use super::*;
    use anyhow::{ensure, Context};

    const SCRIPT: &str = include_str!("platform/linux_atspi.py");

    fn atspi(args: &[&str]) -> Result<String> {
        let path = std::env::temp_dir().join("accessor-atspi.py");
        std::fs::write(&path, SCRIPT).context("Could not write the AT-SPI bridge script")?;
        let output = std::process::Command::new("python3")
            .arg(&path)
            .args(args)
            .output()
            .context("Could not run python3 for the AT-SPI bridge")?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !output.status.success() {
            if !text.is_empty() {
                anyhow::bail!("{text}");
            }
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(text)
    }

    fn wmctrl(args: &[&str]) -> Result<String> {
        let output = std::process::Command::new("wmctrl")
            .args(args)
            .output()
            .context("Could not run wmctrl")?;
        ensure!(output.status.success(), "wmctrl failed");
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn open_app(name: &str) -> Result<String> {
        let name = name.trim();
        ensure!(!name.is_empty(), "open_app needs an app name");
        if std::process::Command::new("gtk-launch")
            .arg(name)
            .spawn()
            .is_ok()
        {
            return Ok(format!("Asked the desktop to launch {name}."));
        }
        if std::process::Command::new(name).spawn().is_ok() {
            return Ok(format!("Launched {name}."));
        }
        std::process::Command::new("xdg-open")
            .arg(name)
            .spawn()
            .with_context(|| format!("Could not open {name}"))?;
        Ok(format!("Opened {name}."))
    }

    pub fn list_windows() -> Result<String> {
        match atspi(&["list"]) {
            Ok(text) if !text.is_empty() => Ok(text),
            _ => wmctrl(&["-l"]).map(|text| {
                if text.is_empty() {
                    "No visible windows found.".to_string()
                } else {
                    text
                }
            }),
        }
    }

    pub fn focus_window(title: &str) -> Result<String> {
        match atspi(&["focus", title]) {
            Ok(text) if !text.starts_with("No window") => Ok(text),
            _ => wmctrl(&["-a", title]).map(|_| format!("Focused {title}.")),
        }
    }

    pub fn ui_snapshot(max: usize) -> Result<String> {
        atspi(&["snapshot", &max.to_string()])
    }
    pub fn ui_invoke(_reference: Option<usize>, name: &str, control_type: &str) -> Result<String> {
        atspi(&["invoke", name, control_type])
    }
    pub fn ui_select(_reference: Option<usize>, name: &str, control_type: &str) -> Result<String> {
        atspi(&["select", name, control_type])
    }
    pub fn ui_expand(_reference: Option<usize>, name: &str, control_type: &str) -> Result<String> {
        atspi(&["expand", name, control_type])
    }
    pub fn ui_set_value(
        _reference: Option<usize>,
        name: &str,
        _control_type: &str,
        text: &str,
    ) -> Result<String> {
        atspi(&["set", name, text])
    }
    pub fn capabilities() -> String {
        let available = std::process::Command::new("python3")
            .args([
                "-c",
                "import gi; gi.require_version('Atspi','2.0'); from gi.repository import Atspi",
            ])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if available {
            "Semantic UI: Linux AT-SPI2 via python3-pyatspi. Enable toolkit accessibility (GNOME: gsettings set org.gnome.desktop.interface toolkit-accessibility true) if trees are empty.".into()
        } else {
            "Semantic UI: Linux needs python3-pyatspi for the accessibility tree; window focus also tries wmctrl. Otherwise use screenshot-based actions.".into()
        }
    }
}

pub use platform::*;

/// Windows Start-menu search, used as the last-resort app launcher.
#[cfg(windows)]
fn start_search(name: &str) -> Result<()> {
    use crate::computer::{key_combo, type_at_focus};
    let mut enigo = crate::computer::input_handle()?;
    // Open Start, type the app name, then launch the top result.
    key_combo(&mut enigo, "win", 1)?;
    std::thread::sleep(std::time::Duration::from_millis(350));
    type_at_focus(name)?;
    std::thread::sleep(std::time::Duration::from_millis(700));
    key_combo(&mut enigo, "Return", 1)?;
    Ok(())
}
