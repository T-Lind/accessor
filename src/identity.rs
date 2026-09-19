//! Spoken reply prefix: harness letter + model letter, then a pause.
pub fn code(harness: &str, model: Option<&str>) -> String {
    let h = match harness {
        "codex" => 'X',
        "claude" => 'C',
        "antigravity" => 'A',
        "mock" => 'M',
        other => other.chars().next().unwrap_or('?').to_ascii_uppercase(),
    };
    let m = model_letter(model.unwrap_or(""));
    format!("{h} {m}")
}
pub fn spoken(harness: &str, model: Option<&str>) -> String {
    let h = match harness {
        "codex" => 'X',
        "claude" => 'C',
        "antigravity" => 'A',
        "mock" => 'M',
        other => other.chars().next().unwrap_or('?').to_ascii_uppercase(),
    };
    let m = model_letter(model.unwrap_or(""));
    format!("{h}. {m}.")
}
pub fn should_announce(last: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    last.map(|t| now.duration_since(t) >= std::time::Duration::from_secs(300))
        .unwrap_or(true)
}
fn model_letter(model: &str) -> char {
    let tail = model
        .rsplit(['-', '/', '.', ' '])
        .find(|p| p.chars().any(|c| c.is_ascii_alphabetic()))
        .unwrap_or(model);
    let lower = tail.to_ascii_lowercase();
    for (needle, letter) in [
        ("sol", 'S'),
        ("astra", 'A'),
        ("fable", 'F'),
        ("opus", 'O'),
        ("sonnet", 'N'),
        ("haiku", 'H'),
        ("flash", 'F'),
        ("pro", 'P'),
        ("luna", 'L'),
        ("gpt", 'G'),
        ("gemini", 'G'),
        ("claude", 'C'),
    ] {
        if lower.contains(needle) {
            return letter;
        }
    }
    tail.chars()
        .find(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('D')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_codes() {
        assert_eq!(code("codex", Some("gpt-5.4-sol")), "X S");
        assert_eq!(code("claude", Some("fable")), "C F");
        assert_eq!(code("antigravity", Some("gemini-3.5-flash")), "A F");
        assert_eq!(code("mock", None), "M D");
        assert_eq!(spoken("antigravity", Some("gemini-3.8-flash")), "A. F.");
        assert_eq!(spoken("codex", Some("gpt-5.4-sol")), "X. S.");
        let t0 = std::time::Instant::now();
        assert!(should_announce(None, t0));
        assert!(!should_announce(
            Some(t0),
            t0 + std::time::Duration::from_secs(60)
        ));
        assert!(should_announce(
            Some(t0),
            t0 + std::time::Duration::from_secs(301)
        ));
    }
}
