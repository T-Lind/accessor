//! Optional single-file master log of everything Accessor does.
//!
//! Disabled by default. When `logging.enabled` is on, the UI layer mirrors every
//! message, chat line, tool line, notice, and ignored transcript here, and the
//! major subsystems add explicit events (routing, agent lifecycle, workers and
//! watches, notifications, memory, organizer, MCP calls, speech). One file,
//! rotated by size, so a whole session can be read in one place.
use crate::config::Settings;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

struct Logger {
    enabled: bool,
    debug: bool,
    path: Option<PathBuf>,
    max_bytes: u64,
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();

fn logger() -> &'static Mutex<Logger> {
    LOGGER.get_or_init(|| {
        Mutex::new(Logger {
            enabled: false,
            debug: false,
            path: None,
            max_bytes: 16 * 1024 * 1024,
        })
    })
}

fn log_path(settings: &Settings) -> anyhow::Result<PathBuf> {
    if let Some(path) = &settings.logging.path {
        return Ok(path.clone());
    }
    Ok(crate::config::home()?.join("logs").join("accessor.log"))
}

/// Apply the current logging settings. Safe to call again after a settings
/// change; the master log turns on or off immediately.
pub fn init(settings: &Settings) {
    let mut logger = logger().lock().unwrap_or_else(|e| e.into_inner());
    logger.enabled = settings.logging.enabled;
    logger.debug = settings.logging.level.eq_ignore_ascii_case("debug");
    logger.max_bytes = settings
        .logging
        .max_mb
        .saturating_mul(1024 * 1024)
        .max(64 * 1024);
    logger.path = if settings.logging.enabled {
        match log_path(settings) {
            Ok(path) => {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                    // The log mirrors transcripts, so keep Accessor-owned log
                    // directories private (custom logging.path values are left
                    // as the user set them).
                    #[cfg(unix)]
                    if crate::config::home()
                        .map(|h| parent.starts_with(&h))
                        .unwrap_or(false)
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
                    }
                }
                // Tighten a log file created by an older build.
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
                }
                Some(path)
            }
            Err(_) => None,
        }
    } else {
        None
    };
    let enabled = logger.enabled;
    drop(logger);
    if enabled {
        event(
            "startup",
            &format!(
                "Accessor {} master log enabled (level {}).",
                env!("CARGO_PKG_VERSION"),
                settings.logging.level
            ),
        );
    }
}

pub fn enabled() -> bool {
    logger()
        .lock()
        .map(|logger| logger.enabled)
        .unwrap_or(false)
}

/// Where the master log is being written, if enabled.
pub fn path() -> Option<PathBuf> {
    logger().lock().ok().and_then(|logger| logger.path.clone())
}

pub fn event(category: &str, message: &str) {
    write("INFO", category, message);
}

pub fn debug(category: &str, message: &str) {
    if logger().lock().map(|logger| logger.debug).unwrap_or(false) {
        write("DEBUG", category, message);
    }
}

fn write(level: &str, category: &str, message: &str) {
    let logger = match logger().lock() {
        Ok(logger) => logger,
        Err(poisoned) => poisoned.into_inner(),
    };
    if !logger.enabled {
        return;
    }
    let Some(path) = logger.path.clone() else {
        return;
    };
    // One line per event: flatten newlines and cap runaway payloads.
    let message: String = message
        .replace(['\r', '\n'], " ")
        .chars()
        .take(4000)
        .collect();
    let line = format!(
        "{} {level:<5} [{category}] {message}\n",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ")
    );
    if let Ok(metadata) = fs::metadata(&path) {
        if metadata.len() >= logger.max_bytes {
            let _ = fs::rename(&path, path.with_extension("1"));
        }
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}
