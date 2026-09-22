//! Narrow, validated settings surface for agents. Credentials, paths and
//! approval policy remain outside this API.
use crate::config::Settings;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
pub const KEYS: &[&str] = &[
    "speak",
    "speak-progress",
    "barge-in",
    "idle-seconds",
    "chat",
    "tts.provider",
    "tts.streaming",
    "tts.voice",
    "tts.local-voice",
    "tts.model",
    "tts.speed",
    "tts.volume",
    "sounds.think",
    "sounds.wake",
    "sounds.sleep",
    "sounds.alarm",
    "routing.main",
    "routing.main-model",
    "routing.reasoning",
    "routing.coding",
    "model",
    "routing.coding-reasoning",
    "agent",
    "routing.plugin-model",
    "routing.plugin-reasoning",
    "routing.plugin-use-main",
    "routing.input-gate",
    "routing.announce",
    "stt.conversation",
    "stt.streaming",
    "stt.noise-gate",
    "stt.denoise",
];
pub fn read(s: &Settings) -> Value {
    let source = serde_json::to_value(s).unwrap();
    let values: serde_json::Map<String, Value> = KEYS
        .iter()
        .map(|key| {
            let mut value = &source;
            for part in key.split('.') {
                value = &value[part.replace('-', "_")];
            }
            ((*key).into(), value.clone())
        })
        .collect();
    let harnesses: Vec<_> = crate::config::harness_offers(s, None)
        .into_iter()
        .map(|h| json!({"id":h.id,"available":h.found,"default_main_model":crate::config::light_model(h.id),"default_worker_model":crate::config::worker_model(h.id)}))
        .collect();
    json!({"values":values,"available_harnesses":harnesses,"guidance":"Read first, then update only explicitly requested preferences. To speak louder or quieter, change tts.volume: 0 is silent, 1 is normal, and 1.5 is maximum. model is the coding model; agent is the plugin harness. default resets a model to its harness default. Change harness and model together. Reasoning: default/low/medium/high. Speed: 0.6–1.5. Other sound volumes: 0–1.5. Plugin-use-main=true inherits main. This does not install plugins, connect accounts or grant permissions. Voice changes affect next playback; harness changes affect the next turn. Worker/standalone changes are saved for the next Accessor launch."})
}
pub fn prepare(current: &Settings, changes: &Value) -> Result<Settings> {
    let changes = changes.as_object().context("changes must be an object")?;
    ensure!(
        !changes.is_empty() && changes.len() <= KEYS.len(),
        "Supply at least one supported setting"
    );
    let mut next = current.clone();
    for (key, value) in changes {
        ensure!(
            KEYS.contains(&key.as_str()),
            "Setting {key} is not agent-editable; use Accessor settings"
        );
        let value = match value {
            Value::String(s) => s.clone(),
            Value::Bool(_) | Value::Number(_) => value.to_string(),
            _ => anyhow::bail!("{key} must be a string, number or boolean"),
        };
        ensure!(
            value.len() <= 256 && !value.chars().any(char::is_control),
            "Invalid setting value"
        );
        next.assign(key, &value)?;
    }
    next.validate()?;
    Ok(next)
}
pub fn save(current: &Settings, changes: &Value) -> Result<Settings> {
    let next = prepare(current, changes)?;
    next.save()?;
    Ok(next)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transactional_validation_and_no_security_settings() {
        let s = Settings::default();
        assert!(prepare(&s, &json!({"tts.speed":1.2,"sounds.alarm":-1})).is_err());
        assert_eq!(s.tts.speed, 1.0);
        for key in [
            "prompt",
            "approvals.reviewer",
            "codex-bin",
            "assets-dir",
            "event-owner",
        ] {
            assert!(prepare(&s, &json!({key:"bad"})).is_err());
        }
        let next=prepare(&s,&json!({"routing.main":"claude","routing.main-model":"haiku","routing.reasoning":"low","tts.speed":1.2,"tts.volume":0.5})).unwrap();
        assert_eq!(next.plugin_target(), ("claude", "haiku", "low"));
        assert_eq!(read(&next)["values"]["tts.volume"], 0.5);
    }
}
