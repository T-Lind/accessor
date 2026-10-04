//! Deliberate voice controls and bounded, local request drafting.
#[derive(Debug, PartialEq)]
pub enum Control {
    Silence,
    ResumeSpeech,
    Status,
    CatchUp,
    Steer(String),
    Draft(String),
    Wait,
    Undo,
    Send,
    Discard,
}
pub fn spoken_progress(text: &str) -> String {
    let (text, _) = crate::organizer::take_directives(text);
    if text.contains("```") || text.contains("http") || text.contains('/') || text.contains('\\') {
        return "Details are on screen.".into();
    }
    text.lines()
        .next()
        .unwrap_or("")
        .split_inclusive(['.', '?', '!'])
        .next()
        .unwrap_or("")
        .chars()
        .take(240)
        .collect()
}
pub fn control(text: &str) -> Option<Control> {
    let text = text.trim().trim_matches(|c: char| c.is_ascii_punctuation());
    let lower = text.to_lowercase();
    Some(match lower.as_str() {
        "stop talking" | "stop speaking" | "quiet please" => Control::Silence,
        "continue speaking" | "resume speaking" => Control::ResumeSpeech,
        "task status" | "what is the task status" | "what's the task status" => Control::Status,
        "catch me up" | "task update" => Control::CatchUp,
        "let me explain" | "start a draft" | "draft a request" => Control::Draft(String::new()),
        "wait" | "hold that thought" => Control::Wait,
        "scratch that" => Control::Undo,
        "send that" | "send the draft" => Control::Send,
        "discard the draft" => Control::Discard,
        _ => {
            for prefix in ["tell the task ", "steer the task ", "for the task "] {
                if lower.starts_with(prefix) {
                    return Some(Control::Steer(text[prefix.len()..].trim().into()));
                }
            }
            if lower.starts_with("let me explain ") {
                return Some(Control::Draft(text[15..].trim().into()));
            }
            return None;
        }
    })
}
#[derive(Default)]
pub struct Draft {
    pub active: bool,
    parts: Vec<String>,
}
impl Draft {
    pub fn begin(&mut self, text: String) -> bool {
        self.active = true;
        self.parts.clear();
        text.is_empty() || self.append(text)
    }
    pub fn append(&mut self, text: String) -> bool {
        if self.parts.iter().map(|p| p.len() + 1).sum::<usize>() + text.len() > 32_000
            || self.parts.len() >= 100
        {
            return false;
        }
        self.parts.push(text);
        true
    }
    pub fn undo(&mut self) {
        self.parts.pop();
    }
    pub fn take(&mut self) -> Option<String> {
        let text = self.parts.join(" ");
        if text.trim().is_empty() {
            return None;
        }
        self.clear();
        Some(text)
    }
    pub fn clear(&mut self) {
        self.active = false;
        self.parts.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_are_deliberate_and_drafts_are_bounded() {
        assert_eq!(control("Stop talking."), Some(Control::Silence));
        assert_eq!(control("write a function to stop talking"), None);
        let mut d = Draft::default();
        d.begin("Research batteries.".into());
        d.append("Use vendor papers.".into());
        d.undo();
        d.append("Use primary sources.".into());
        assert!(!d.append("x".repeat(32_001)));
        assert_eq!(
            d.take().as_deref(),
            Some("Research batteries. Use primary sources.")
        );
        assert!(!d.active);
    }
}
