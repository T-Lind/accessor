//! Lifetime and rolling-week usage. Costs are estimates for the dashboard.
use crate::config;
use serde::{Deserialize, Serialize};
use std::{
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
pub struct Totals {
    pub tts_calls: u64,
    pub tts_chars: u64,
    pub stt_calls: u64,
    pub harness_turns: u64,
    pub compact_calls: u64,
    pub jev_calls: u64,
    pub usd: f64,
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
        })
}

fn save(store: &Store) {
    if let Some(p) = path() {
        let _ = config::save_private(&p, &serde_json::to_vec(store).unwrap_or_default());
    }
}

fn store() -> &'static Mutex<Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
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

fn record(kind: &str, label: &str, units: f64, usd: f64) {
    let Ok(mut store) = store().lock() else {
        return;
    };
    let ts = now();
    bump(kind, &mut store.lifetime);
    if kind == "tts" {
        store.lifetime.tts_chars += units.max(0.0) as u64;
    }
    store.lifetime.usd += usd;
    store.events.push(Event {
        ts,
        kind: kind.into(),
        label: label.into(),
        units,
        usd,
    });
    let cutoff = ts.saturating_sub(WEEK * 4);
    store.events.retain(|e| e.ts >= cutoff);
    if store.events.len() > MAX_EVENTS {
        let extra = store.events.len() - MAX_EVENTS;
        store.events.drain(..extra);
    }
    save(&store);
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
        bump(&e.kind, &mut t);
        t.usd += e.usd;
        if e.kind == "tts" {
            t.tts_chars += e.units.max(0.0) as u64;
        }
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

fn section(title: &str, t: &Totals) -> String {
    let rows = [
        ("Jev", t.jev_calls as f64),
        ("TTS", t.tts_calls as f64),
        ("STT", t.stt_calls as f64),
        ("Harness", t.harness_turns as f64),
        ("Compact", t.compact_calls as f64),
    ];
    let max = rows.iter().map(|(_, n)| *n).fold(0.0, f64::max);
    let mut out = format!("{title}\n  estimated ${:.4}\n", t.usd);
    for (name, n) in rows {
        out.push_str(&format!("  {name:<8} {} {n:.0}\n", bar(n, max)));
    }
    out.push_str(&format!(
        "  TTS chars {} · Jev {} · turns {}\n",
        t.tts_chars, t.jev_calls, t.harness_turns
    ));
    out
}

pub fn report() -> String {
    let Ok(store) = store().lock() else {
        return "Analytics unavailable.".into();
    };
    let week = week_totals(&store.events, now().saturating_sub(WEEK));
    format!(
        "Accessor analytics (estimates; Cartesia/Jev/Gateway list prices, harness tokens unpriced)\n\n{}{}",
        section("Lifetime", &store.lifetime),
        section("Past 7 days", &week)
    )
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
    fn tokens_are_rough_char_quarters() {
        assert_eq!(approx_tokens("abcd"), 1);
        assert_eq!(approx_tokens("abcde"), 2);
    }
}
