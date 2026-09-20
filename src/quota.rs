//! Subscription quotas. These are provider observations, never cost estimates.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bucket {
    pub name: String,
    pub used_percent: f64,
    pub remaining_percent: f64,
    pub resets_at: Option<u64>,
    pub reset_time: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reading {
    pub harness: String,
    pub source: String,
    pub observed_at: u64,
    pub buckets: Vec<Bucket>,
    pub note: Option<String>,
}
fn now() -> u64 {
    crate::organizer::now_unix()
}
fn bucket(
    name: String,
    used: Option<f64>,
    reset: Option<u64>,
    iso: Option<String>,
) -> Option<Bucket> {
    let used = used.filter(|n| n.is_finite() && (0.0..=100.0).contains(n))?;
    Some(Bucket {
        name,
        used_percent: used,
        remaining_percent: 100.0 - used,
        resets_at: reset,
        reset_time: iso,
    })
}
pub fn parse(harness: &str, value: &Value) -> Vec<Bucket> {
    let mut out = Vec::new();
    match harness {
        "codex" => {
            let data = value.get("result").unwrap_or(value);
            let rates: Vec<(String, &Value)> = if let Some(map) = data["rateLimitsByLimitId"]
                .as_object()
                .filter(|m| !m.is_empty())
            {
                map.iter().map(|(k, v)| (k.clone(), v)).collect()
            } else {
                vec![("codex".into(), &data["rateLimits"])]
            };
            for (id, rate) in rates {
                for key in ["primary", "secondary"] {
                    let w = &rate[key];
                    let period = match w["windowDurationMins"].as_u64() {
                        Some(300) => "5 hours".into(),
                        Some(10080) => "weekly".into(),
                        Some(n) => format!("{n} minutes"),
                        None => key.into(),
                    };
                    if let Some(b) = bucket(
                        format!("{} · {period}", rate["limitName"].as_str().unwrap_or(&id)),
                        w["usedPercent"].as_f64(),
                        w["resetsAt"].as_u64(),
                        None,
                    ) {
                        out.push(b);
                    }
                }
            }
        }
        "claude" => {
            if let Some(rates) = value["rate_limits"].as_object() {
                for (key, w) in rates {
                    let iso = w["resets_at"].as_str().map(str::to_owned);
                    if let Some(b) = bucket(
                        key.replace('_', " "),
                        w["used_percentage"]
                            .as_f64()
                            .or_else(|| w["utilization"].as_f64()),
                        w["resets_at"]
                            .as_u64()
                            .or_else(|| iso.as_deref().and_then(iso_epoch)),
                        iso,
                    ) {
                        out.push(b);
                    }
                }
                for w in value["rate_limits"]["model_scoped"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    let iso = w["resets_at"].as_str().map(str::to_owned);
                    if let Some(b) = bucket(
                        w["display_name"].as_str().unwrap_or("model weekly").into(),
                        w["utilization"].as_f64(),
                        iso.as_deref().and_then(iso_epoch),
                        iso,
                    ) {
                        out.push(b);
                    }
                }
            }
        }
        "antigravity" => {
            if let Some(rates) = value["quota"].as_object() {
                for (key, w) in rates {
                    let iso = w["reset_time"].as_str().map(str::to_owned);
                    let reset = iso.as_deref().and_then(iso_epoch).or_else(|| {
                        w["reset_in_seconds"]
                            .as_u64()
                            .and_then(|s| now().checked_add(s))
                    });
                    if let Some(b) = bucket(
                        key.clone(),
                        w["remaining_fraction"].as_f64().map(|n| 100.0 - n * 100.0),
                        reset,
                        iso,
                    ) {
                        out.push(b);
                    }
                }
            }
        }
        _ => {}
    }
    out
}
// AGY's documented report currently uses UTC ISO-8601 seconds. Refuse other
// formats instead of inventing a reset. Preserve the original text regardless.
fn iso_epoch(s: &str) -> Option<u64> {
    let s = s.strip_suffix('Z').or_else(|| s.strip_suffix("+00:00"))?;
    let (date, time) = s.split_once('T')?;
    let d: Vec<i64> = date
        .split('-')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    let t: Vec<i64> = time
        .split('.')
        .next()?
        .split(':')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    if d.len() != 3
        || t.len() != 3
        || !(1970..=9999).contains(&d[0])
        || !(1..=12).contains(&d[1])
        || !(1..=31).contains(&d[2])
        || !(0..24).contains(&t[0])
        || !(0..60).contains(&t[1])
        || !(0..60).contains(&t[2])
    {
        return None;
    }
    let leap = d[0] % 4 == 0 && (d[0] % 100 != 0 || d[0] % 400 == 0);
    let max_day = match d[1] {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if d[2] > max_day {
        return None;
    }
    let y = d[0] - i64::from(d[1] <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = d[1] + if d[1] > 2 { -3 } else { 9 };
    let days =
        era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + (153 * m + 2) / 5 + d[2] - 1 - 719468;
    u64::try_from(days * 86400 + t[0] * 3600 + t[1] * 60 + t[2]).ok()
}
fn utc_date(epoch: u64) -> String {
    let days = (epoch / 86400).min(i64::MAX as u64 - 719468) as i64 + 719468;
    let era = days.div_euclid(146097);
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        epoch % 86400 / 3600,
        epoch % 3600 / 60
    )
}
fn parse_agy(text: &str) -> Vec<Bucket> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split('\t').map(str::trim).collect();
            if fields.len() != 4 || !fields[1].ends_with("Limit Remaining") {
                return None;
            }
            let remaining = fields[2].strip_suffix('%')?.parse::<f64>().ok()?;
            bucket(
                format!(
                    "{} · {}",
                    fields[0],
                    fields[1].trim_end_matches(" Remaining")
                ),
                Some(100.0 - remaining),
                iso_epoch(fields[3]),
                Some(fields[3].into()),
            )
        })
        .collect()
}
fn path(harness: &str) -> Result<PathBuf> {
    ensure!(
        ["codex", "claude", "antigravity"].contains(&harness),
        "Unknown quota harness"
    );
    Ok(crate::config::home()?.join(format!("quota-{harness}.json")))
}
fn save(reading: &Reading) -> Result<()> {
    crate::config::save_private(&path(&reading.harness)?, &serde_json::to_vec(reading)?)
}
pub fn ingest(harness: &str, value: &Value) -> Result<Reading> {
    let buckets = parse(harness, value);
    ensure!(
        !buckets.is_empty(),
        "No subscription quota fields in this payload; prior reading preserved"
    );
    let reading = Reading {
        harness: harness.into(),
        source: if harness == "codex" {
            "app-server"
        } else {
            "status-line / harness event"
        }
        .into(),
        observed_at: now(),
        buckets,
        note: None,
    };
    save(&reading)?;
    Ok(reading)
}
pub fn observe(harness: &str, value: &Value) {
    if !parse(harness, value).is_empty() {
        let _ = ingest(harness, value);
    }
}
fn cached(harness: &str) -> Reading {
    path(harness).ok().and_then(|p| std::fs::read(p).ok()).and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_else(||Reading {
        harness:harness.into(),source:"unavailable".into(),observed_at:0,buckets:Vec::new(),note:Some(if harness=="claude" {
            "No quota sample yet. Claude exposes rate_limits through its status line after a request; feed that JSON to acc usage --ingest claude. Headless mode may not emit it."
        } else {"No quota reading yet."}.into()),
    })
}
async fn codex(executable: PathBuf) -> Result<Reading> {
    let mut cmd = Command::new(executable);
    cmd.arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut process = crate::process_tree::spawn(&mut cmd)?;
    let _tree = crate::process_tree::ProcessTree::attach(&process)?;
    let mut stdin = process.stdin.take().context("Missing quota input")?;
    let mut output = BufReader::new(process.stdout.take().context("Missing quota output")?);
    let result=tokio::time::timeout(Duration::from_secs(15),async {
        stdin.write_all(format!("{}\n",json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"accessor_quota","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}})).as_bytes()).await?;
        loop {
            let mut line=Vec::new();
            let n=(&mut output).take(1_048_577).read_until(b'\n',&mut line).await?;
            ensure!(n>0 && n<=1_048_576,"Quota connection closed or frame too large");
            let v:Value=serde_json::from_slice(&line)?;
            if v["id"]==1 {
                ensure!(v.get("error").is_none(),"Codex quota initialization failed");
                stdin.write_all(b"{\"method\":\"initialized\"}\n{\"id\":2,\"method\":\"account/rateLimits/read\",\"params\":{}}\n").await?;
            } else if v["id"]==2 {
                ensure!(v.get("error").is_none(),"Codex quota unavailable; check login and CLI version");
                return ingest("codex",&v);
            }
        }
    }).await.context("Codex quota refresh timed out")?;
    let _ = process.kill().await;
    result
}
async fn agy(executable: PathBuf) -> Result<Reading> {
    let mut cmd = Command::new(executable);
    cmd.args(["-p", "/usage", "--print-timeout", "12s"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut process = crate::process_tree::spawn(&mut cmd)?;
    let _tree = crate::process_tree::ProcessTree::attach(&process)?;
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        let mut bytes = Vec::new();
        process
            .stdout
            .take()
            .context("Missing quota output")?
            .take(65537)
            .read_to_end(&mut bytes)
            .await?;
        ensure!(bytes.len() <= 65536, "Quota report too large");
        let status = process.wait().await?;
        ensure!(
            status.success(),
            "Antigravity quota unavailable; check native login"
        );
        let buckets = parse_agy(&String::from_utf8_lossy(&bytes));
        ensure!(
            !buckets.is_empty(),
            "Antigravity returned no recognized quota rows; check native /usage"
        );
        let reading = Reading {
            harness: "antigravity".into(),
            source: "agy -p /usage".into(),
            observed_at: now(),
            buckets,
            note: None,
        };
        save(&reading)?;
        Ok(reading)
    })
    .await
    .context("Antigravity quota refresh timed out; check native login")?;
    let _ = process.kill().await;
    result
}
async fn claude(executable: PathBuf) -> Result<Reading> {
    let mut cmd = Command::new(executable);
    cmd.args([
        "--print",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true);
    let mut process = crate::process_tree::spawn(&mut cmd)?;
    let _tree = crate::process_tree::ProcessTree::attach(&process)?;
    let mut stdin = process.stdin.take().context("Missing Claude quota input")?;
    let mut output = BufReader::new(
        process
            .stdout
            .take()
            .context("Missing Claude quota output")?,
    );
    let result=tokio::time::timeout(Duration::from_secs(15),async {
        stdin.write_all(b"{\"type\":\"control_request\",\"request_id\":\"init\",\"request\":{\"subtype\":\"initialize\"}}\n").await?;
        loop {
            let mut line=Vec::new();
            let n=(&mut output).take(1_048_577).read_until(b'\n',&mut line).await?;
            ensure!(n>0 && n<=1_048_576,"Claude quota connection closed or frame too large");
            let v:Value=serde_json::from_slice(&line)?;
            if v["type"]!="control_response" {continue;}
            let r=&v["response"];
            ensure!(r["subtype"]=="success","Claude structured usage unsupported or unavailable; status-line capture can supply a reading");
            if r["request_id"]=="init" {
                stdin.write_all(b"{\"type\":\"control_request\",\"request_id\":\"quota\",\"request\":{\"subtype\":\"get_usage\"}}\n").await?;
            } else if r["request_id"]=="quota" {
                let data=&r["response"];
                let buckets=parse("claude",data);
                ensure!(!buckets.is_empty() || data["rate_limits_available"]==false,"Claude quota response format unrecognized; previous reading preserved");
                let reading=Reading{harness:"claude".into(),source:"Claude get_usage (experimental)".into(),observed_at:now(),buckets,note:if data["rate_limits_available"]==false {Some("This Claude session reports plan quotas unavailable (for example API-key authentication or missing subscription scope).".into())}else{None}};
                save(&reading)?;return Ok(reading);
            }
        }
    }).await.context("Claude quota refresh timed out")?;
    let _ = process.kill().await;
    result
}
pub async fn snapshot(refresh: bool) -> Result<Value> {
    let settings = crate::config::Settings::load()?;
    let mut readings = vec![cached("codex"), cached("claude"), cached("antigravity")];
    if refresh {
        let (c, a, cl) = tokio::join!(
            codex(crate::config::codex(&settings)),
            agy(crate::config::harness_bin("antigravity", &settings, None)),
            claude(crate::config::harness_bin("claude", &settings, None))
        );
        for (i, r) in [(0, c), (2, a), (1, cl)] {
            match r {
                Ok(v) => readings[i] = v,
                Err(e) => readings[i].note = Some(format!("Refresh failed: {e:#}")),
            }
        }
    }
    let ts = now();
    let providers: Vec<_> = readings
        .into_iter()
        .map(|r| {
            let age = ts.saturating_sub(r.observed_at);
            let expired = r
                .buckets
                .iter()
                .any(|b| b.resets_at.is_some_and(|t| t <= ts));
            let stale = !r.buckets.is_empty()
                && r.observed_at > 0
                && (age > 300 || expired || r.note.is_some());
            let mut v = serde_json::to_value(&r).unwrap();
            v["available"] = json!(!r.buckets.is_empty());
            v["stale"] = json!(stale);
            v["age_seconds"] = if r.observed_at == 0 {
                Value::Null
            } else {
                json!(age)
            };
            for b in v["buckets"].as_array_mut().unwrap() {
                b["reset_in_seconds"] = b["resets_at"]
                    .as_u64()
                    .map(|n| json!(n.saturating_sub(ts)))
                    .unwrap_or(Value::Null);
            }
            v
        })
        .collect();
    Ok(
        json!({"checked_at":ts,"providers":providers,"note":"Subscription quotas, not token costs. Missing values are unknown; stale values are historical. Refresh does not run model turns. /analytics shows estimated costs; /limits shows local retry delays."}),
    )
}
pub fn report(v: &Value) -> String {
    let mut lines = vec!["SUBSCRIPTION USAGE · bars show consumed quota".into()];
    for p in v["providers"].as_array().into_iter().flatten() {
        lines.push(format!(
            "\n{} · {}{}",
            p["harness"].as_str().unwrap_or("?"),
            p["source"].as_str().unwrap_or("?"),
            if p["stale"] == true { " · STALE" } else { "" }
        ));
        if let Some(age) = p["age_seconds"].as_u64() {
            lines.push(format!("Observed {age}s ago"));
        }
        if p["available"] != true {
            lines.push("  Usage unavailable — no percentage reported".into());
        }
        for b in p["buckets"].as_array().into_iter().flatten() {
            let used = b["used_percent"].as_f64().unwrap_or(0.0);
            let n = (used / 5.0).round().clamp(0.0, 20.0) as usize;
            lines.push(format!(
                "  {}\n  [{}{}] {:.0}% used · {:.0}% left",
                crate::ui::safe(b["name"].as_str().unwrap_or("quota")),
                "█".repeat(n),
                "░".repeat(20 - n),
                used,
                100.0 - used
            ));
            let reset = b["reset_time"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| b["resets_at"].as_u64().map(utc_date))
                .unwrap_or_else(|| "unknown".into());
            let countdown = b["reset_in_seconds"]
                .as_u64()
                .map(|s| {
                    if s == 0 {
                        " · reset passed; refresh needed".into()
                    } else {
                        format!(" · in {}h {}m", s / 3600, (s % 3600) / 60)
                    }
                })
                .unwrap_or_default();
            lines.push(format!("  Resets: {reset}{countdown}"));
        }
        if let Some(note) = p["note"].as_str() {
            lines.push(crate::ui::safe(note));
        }
    }
    lines.push("\n/usage refreshes supported providers. /analytics: costs & latency. /limits: retry delays.".into());
    lines.join("\n")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalize_provider_fields_without_inventing_missing_values() {
        let v = json!({"rateLimits":{"primary":{"usedPercent":99}},"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":25,"windowDurationMins":300,"resetsAt":2000000000},"secondary":null},"other":{"primary":{"usedPercent":10}}}});
        let rows = parse("codex", &v);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].remaining_percent, 75.0);
        assert!(parse("claude", &json!({"context_window":{"used_percentage":33}})).is_empty());
        assert_eq!(parse("claude",&json!({"rate_limits":{"five_hour":{"used_percentage":23,"resets_at":2000000000},"seven_day":null}}))[0].remaining_percent,77.0);
        assert!(bucket("bad".into(), Some(101.0), None, None).is_none());
    }
    #[test]
    fn agy_tsv_and_statusline_agree() {
        let rows=parse_agy("Gemini Models\tWeekly Limit Remaining\t72%\t2026-09-26T06:15:29Z\nAuthentication required");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].used_percent, 28.0);
        assert_eq!(iso_epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_epoch("2026-02-30T00:00:00Z"), None);
        assert_eq!(iso_epoch("9223372036854775807-01-01T00:00:00Z"), None);
        assert_eq!(utc_date(0), "1970-01-01 00:00 UTC");
        assert_eq!(iso_epoch("2026-09-26T06:15:29Z"), Some(1790403329));
        let data = json!({"quota":{"gemini-weekly":{"remaining_fraction":0.72,"reset_time":"2026-09-26T06:15:29Z"}}});
        assert_eq!(parse("antigravity", &data)[0].resets_at, rows[0].resets_at);
    }
}
