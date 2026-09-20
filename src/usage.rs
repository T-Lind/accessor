//! Lifetime and rolling-week usage. Costs are estimates for the dashboard.
use crate::config;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const WEEK: u64 = 7 * 24 * 3600;
const MAX_EVENTS: usize = 8_000;
const TTS_USD_PER_1K_CHARS: f64 = 0.025;
const STT_USD_PER_MIN: f64 = 0.006;
const JEV_USD: f64 = 0.0002;
const GATEWAY_IN_PER_M: f64 = 0.15;
const GATEWAY_OUT_PER_M: f64 = 0.60;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Totals {
    pub tts_calls: u64,
    pub tts_chars: u64,
    pub stt_calls: u64,
    pub harness_turns: u64,
    pub compact_calls: u64,
    pub jev_calls: u64,
    pub usd: f64,
    pub diagnostics: BTreeMap<String, u64>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Event {
    ts: u64,
    kind: String,
    label: String,
    units: f64,
    usd: f64,
}

#[derive(Clone, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    lifetime: Totals,
    #[serde(default)]
    events: Vec<Event>,
    #[serde(skip)]
    session: Vec<Event>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn path() -> Option<std::path::PathBuf> {
    config::home().ok().map(|h| h.join("analytics.json"))
}

fn load() -> Store {
    path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| Store {
            lifetime: Totals::default(),
            events: Vec::new(),
            session: Vec::new(),
        })
}

fn save(store: &Store) {
    if let Some(p) = path() {
        let _ = config::save_private(&p, &serde_json::to_vec(store).unwrap_or_default());
    }
}

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
fn store() -> &'static Mutex<Store> {
    STORE.get_or_init(|| Mutex::new(load()))
}

fn bump(kind: &str, totals: &mut Totals) {
    match kind {
        "tts" => totals.tts_calls += 1,
        "stt" => totals.stt_calls += 1,
        "harness" => totals.harness_turns += 1,
        "compact" => totals.compact_calls += 1,
        "jev" => totals.jev_calls += 1,
        _ => {}
    }
}

fn accumulate(event: &Event, totals: &mut Totals) {
    bump(&event.kind, totals);
    if event.kind == "tts" {
        totals.tts_chars += event.units.max(0.0) as u64;
    }
    if event.kind == "diagnostic" {
        *totals.diagnostics.entry(event.label.clone()).or_default() += 1;
    }
    totals.usd += event.usd;
}

fn record(kind: &str, label: &str, units: f64, usd: f64) {
    let Ok(mut store) = store().lock() else {
        return;
    };
    let ts = now();
    let event = Event {
        ts,
        kind: kind.into(),
        label: label.into(),
        units,
        usd,
    };
    accumulate(&event, &mut store.lifetime);
    store.session.push(event.clone());
    if store.session.len() > MAX_EVENTS {
        store.session.remove(0);
    }
    store.events.push(event);
    store.events.retain(|e| e.ts >= ts.saturating_sub(WEEK * 4));
    if store.events.len() > MAX_EVENTS {
        let extra = store.events.len() - MAX_EVENTS;
        store.events.drain(..extra);
    }
    drop(store);
    // Avoid a disk write on the recognition/UI path for each timing sample.
    static WRITER: OnceLock<std::sync::mpsc::SyncSender<()>> = OnceLock::new();
    let tx = WRITER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                std::thread::sleep(std::time::Duration::from_millis(200));
                while rx.try_recv().is_ok() {}
                flush();
            }
        });
        tx
    });
    let _ = tx.try_send(());
}

pub fn flush() {
    static WRITE_LOCK: Mutex<()> = Mutex::new(());
    let Ok(_write) = WRITE_LOCK.lock() else {
        return;
    };
    let snapshot = STORE
        .get()
        .and_then(|store| store.lock().ok().map(|s| s.clone()));
    if let Some(snapshot) = snapshot {
        save(&snapshot);
    }
}

pub fn record_latency(stage: &str, elapsed: std::time::Duration) {
    record("latency", stage, elapsed.as_secs_f64() * 1000.0, 0.0);
}
pub fn record_diagnostic(label: &str) {
    record("diagnostic", label, 1.0, 0.0);
}

pub fn approx_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

pub fn record_tts(provider: &str, chars: usize) {
    let usd = if provider == "cartesia" {
        chars as f64 / 1000.0 * TTS_USD_PER_1K_CHARS
    } else {
        0.0
    };
    record("tts", provider, chars as f64, usd);
}

pub fn record_stt(provider: &str, seconds: f64) {
    let usd = if provider == "cartesia" {
        seconds / 60.0 * STT_USD_PER_MIN
    } else {
        0.0
    };
    record("stt", provider, seconds, usd);
}

pub fn record_harness(harness: &str, tokens: usize) {
    record("harness", harness, tokens as f64, 0.0);
}

pub fn record_compact(model: &str, tokens: usize) {
    let usd = tokens as f64 / 1_000_000.0 * (GATEWAY_IN_PER_M + GATEWAY_OUT_PER_M) / 2.0;
    record("compact", model, tokens as f64, usd);
}

pub fn record_jev(ok: bool) {
    record(
        "jev",
        if ok { "systemone" } else { "systemone-fail" },
        1.0,
        JEV_USD,
    );
}

fn week_totals(events: &[Event], since: u64) -> Totals {
    let mut t = Totals::default();
    for e in events.iter().filter(|e| e.ts >= since) {
        accumulate(e, &mut t);
    }
    t
}

fn bar(value: f64, max: f64) -> String {
    let width = 16usize;
    if max <= 0.0 {
        return "░".repeat(width);
    }
    let fill = ((value / max) * width as f64).round() as usize;
    let fill = fill.min(width);
    format!("{}{}", "█".repeat(fill), "░".repeat(width - fill))
}

fn percentile(values: &mut [f64], percent: usize) -> f64 {
    values.sort_by(f64::total_cmp);
    if values.is_empty() {
        return 0.0;
    }
    values[(values.len() * percent)
        .div_ceil(100)
        .saturating_sub(1)
        .min(values.len() - 1)]
}

fn render_report(store: &Store, at: u64) -> String {
    let week_events: Vec<_> = store
        .events
        .iter()
        .filter(|e| e.ts >= at.saturating_sub(WEEK))
        .cloned()
        .collect();
    let week = week_totals(&week_events, 0);
    let today = week_totals(&store.events, at.saturating_sub(24 * 3600));
    let session = week_totals(&store.session, 0);
    let mut out=String::from("ACCESSOR ANALYTICS\nUsage, speech decisions and latency\n\nWindow           Est. cost    Agent turns   STT calls   TTS calls\n");
    for (name, t) in [
        ("Lifetime", &store.lifetime),
        ("Past 7 days", &week),
        ("Past 24 hours", &today),
        ("This run", &session),
    ] {
        out.push_str(&format!(
            "{name:<16} ${:<11.4} {:<13} {:<11} {}\n",
            t.usd, t.harness_turns, t.stt_calls, t.tts_calls
        ));
    }
    out.push_str("\nServices · past 7 days\n");
    let mut services: BTreeMap<(String, String), (usize, f64)> = BTreeMap::new();
    for e in &week_events {
        if ["tts", "stt", "harness", "compact", "jev"].contains(&e.kind.as_str()) {
            let row = services
                .entry((e.kind.clone(), e.label.clone()))
                .or_default();
            row.0 += 1;
            row.1 += e.usd;
        }
    }
    let maximum = services.values().map(|v| v.0).max().unwrap_or(0) as f64;
    for (kind, title) in [
        ("jev", "Jev"),
        ("stt", "STT"),
        ("tts", "TTS"),
        ("harness", "Harness"),
        ("compact", "Compaction"),
    ] {
        let mut found = false;
        for ((service, label), (calls, usd)) in &services {
            if service == kind {
                found = true;
                out.push_str(&format!(
                    "{title:<11} {label:<20} {} {calls:>5}  ${usd:.4}\n",
                    bar(*calls as f64, maximum)
                ));
            }
        }
        if !found {
            out.push_str(&format!("{title:<11} no calls recorded\n"));
        }
    }
    out.push_str("\nSpeech health · this run / past 7 days\n");
    for label in [
        "Voice accepted",
        "Voice ignored",
        "No words recognized",
        "Cloud STT fallback",
        "gate fallback",
        "TTS cache hit",
        "Streaming STT clip",
    ] {
        out.push_str(&format!(
            "{label:<25} {:>6} / {}\n",
            session.diagnostics.get(label).unwrap_or(&0),
            week.diagnostics.get(label).unwrap_or(&0)
        ));
    }
    out.push_str("\nLatency · past 7 days (measured; ms)\nStage                      Samples     Median        P95\n");
    let mut timings: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for e in &week_events {
        if e.kind == "latency" && e.units.is_finite() {
            timings.entry(&e.label).or_default().push(e.units);
        }
    }
    if timings.is_empty() {
        out.push_str("No timing samples yet. Speak a request to start measuring.\n");
    }
    for (stage, mut values) in timings {
        let median = percentile(&mut values, 50);
        let p95 = percentile(&mut values, 95);
        out.push_str(&format!(
            "{stage:<27}{:>7} {median:>10.0} {p95:>10.0}\n",
            values.len()
        ));
    }
    out.push_str("\nCosts use built-in estimates, not invoices or live account quotas.\nHarness tokens are approximate input only and unpriced; legacy STT totals may include duplicate counts.\nRecent details retain up to 8,000 events / 28 days; busy periods may be incomplete.\nIgnored words appear only in Activity. Analytics stores counts and timings, never transcript text.\n");
    out
}
pub fn report() -> String {
    let Ok(store) = store().lock() else {
        return "Analytics unavailable.".into();
    };
    render_report(&store, now())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn week_excludes_old_jev_and_keeps_recent() {
        let now = 2_000_000u64;
        let events = vec![
            Event {
                ts: now - WEEK - 10,
                kind: "jev".into(),
                label: "old".into(),
                units: 1.0,
                usd: 0.01,
            },
            Event {
                ts: now - 60,
                kind: "jev".into(),
                label: "new".into(),
                units: 1.0,
                usd: 0.0002,
            },
            Event {
                ts: now - 60,
                kind: "tts".into(),
                label: "cartesia".into(),
                units: 80.0,
                usd: 0.002,
            },
        ];
        let week = week_totals(&events, now - WEEK);
        assert_eq!(week.jev_calls, 1);
        assert_eq!(week.tts_calls, 1);
        assert!(week.usd < 0.01);
    }
    #[test]
    fn report_has_latency_health_and_migrates_old_totals() {
        let mut store: Store =
            serde_json::from_str(r#"{"lifetime":{"stt_calls":7},"events":[]}"#).unwrap();
        let at = 2_000_000;
        for milliseconds in [10.0, 20.0, 100.0] {
            store.events.push(Event {
                ts: at,
                kind: "latency".into(),
                label: "Relevance check".into(),
                units: milliseconds,
                usd: 0.0,
            });
        }
        let event = Event {
            ts: at,
            kind: "diagnostic".into(),
            label: "Voice ignored".into(),
            units: 1.0,
            usd: 0.0,
        };
        store.events.push(event.clone());
        store.session.push(event.clone());
        accumulate(&event, &mut store.lifetime);
        let report = render_report(&store, at);
        assert!(report.contains("Voice ignored"));
        assert!(report.contains("Relevance check"));
        assert!(report.contains("P95"));
        assert_eq!(store.lifetime.stt_calls, 7);
        assert_eq!(percentile(&mut [100.0, 10.0, 20.0], 50), 20.0);
        assert_eq!(percentile(&mut [100.0, 10.0, 20.0], 95), 100.0);
        let saved = serde_json::to_value(&store).unwrap();
        assert!(saved.get("session").is_none());
    }

    #[test]
    fn tokens_are_rough_char_quarters() {
        assert_eq!(approx_tokens("abcd"), 1);
        assert_eq!(approx_tokens("abcde"), 2);
    }
}
