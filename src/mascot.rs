//! A full-size animated bear whose expression mirrors Accessor's state.
//!
//! The mood is normally chosen automatically from the live phase (asleep,
//! listening, thinking, speaking, alarmed...). The agent can also steer it by
//! emitting a directive such as `[[mood:happy]]` inside a reply — the same idea
//! as the OpenAI realtime `set_emotion` pattern — which is parsed and removed
//! before the text is shown or spoken. The local openJev decision model reads
//! reply tone off the hot path, with hosted Jev as an optional fallback.
//!
//! The renderer and phase mapping are local; model inference runs separately
//! so it never adds latency to the speech or UI hot path.

pub mod bear;

use ratatui::style::Color;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Asleep,
    Idle,
    Listening,
    Thinking,
    Speaking,
    Happy,
    Delighted,
    Proud,
    Excited,
    Sad,
    Concerned,
    Confused,
    Apologetic,
    Cautious,
    Alarmed,
    Worried,
    Locked,
    Waving,
    Curious,
    Working,
    Focused,
    Surprised,
    Sleepy,
    Celebrating,
}

impl Mood {
    /// The short human name shown in the mascot's title bar.
    pub fn label(self) -> &'static str {
        match self {
            Self::Asleep => "asleep",
            Self::Idle => "idle",
            Self::Listening => "listening",
            Self::Thinking => "thinking",
            Self::Speaking => "speaking",
            Self::Happy => "happy",
            Self::Delighted => "delighted",
            Self::Proud => "proud",
            Self::Excited => "excited",
            Self::Sad => "sad",
            Self::Concerned => "concerned",
            Self::Confused => "confused",
            Self::Apologetic => "sorry",
            Self::Cautious => "cautious",
            Self::Alarmed => "alarm!",
            Self::Worried => "worried",
            Self::Locked => "locked",
            Self::Waving => "hi!",
            Self::Curious => "curious",
            Self::Working => "working",
            Self::Focused => "focused",
            Self::Surprised => "surprised",
            Self::Sleepy => "sleepy",
            Self::Celebrating => "celebrating",
        }
    }

    /// A three-line face. Lines are rendered centered, so ragged width is fine.
    pub fn face(self) -> [&'static str; 3] {
        match self {
            Self::Asleep => ["  .----.  ", " ( -  - ) ", "  ' .. ' z"],
            Self::Idle => ["  .----.  ", " ( o  o ) ", "  ' .. '  "],
            Self::Listening | Self::Curious => ["  .----.  ", " ( O  O ) ", "  \\____/  "],
            Self::Thinking | Self::Working => ["  .----.  ", " ( o  o ) ", "  ' ~~ ' ."],
            Self::Speaking => ["  .----.  ", " ( O  O ) ", "  ' \\/ '  "],
            Self::Happy => ["  .----.  ", " ( ^  ^ ) ", "  ' \\/ '  "],
            Self::Delighted | Self::Celebrating => ["  .----.  ", " ( ^  ^ ) ", "  ' \\/ '* "],
            Self::Proud => ["  .----.  ", " ( ^  ^ ) ", "  ' \\/ ' +"],
            Self::Excited | Self::Surprised => ["  .----.  ", " ( ^  o ) ", "  ' \\/ ' !"],
            Self::Sad => ["  .----.  ", " ( ;  ; ) ", "  ' /\\ '  "],
            Self::Concerned => ["  .----.  ", " ( o  o ) ", "  ' /\\ '  "],
            Self::Confused => ["  .----.  ", " ( o  O ) ", "  ' ~  ' ?"],
            Self::Apologetic => ["  .----.  ", " ( >  < ) ", "  ' /\\ ' '"],
            Self::Cautious | Self::Focused | Self::Sleepy => {
                ["  .----.  ", " ( -  - ) ", "  ' -- '  "]
            }
            Self::Alarmed => ["  .----.  ", " ( O  O ) ", "  ' /\\ '!!"],
            Self::Worried => ["  .----.  ", " ( o  o ) ", "  ' .. '  "],
            Self::Locked => ["  .----.  ", " ( x  x ) ", "  ' -- ' #"],
            Self::Waving => ["  .----.\\o", " ( ^  ^ ) ", "  ' \\/ '  "],
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Asleep => Color::DarkGray,
            Self::Idle => Color::Gray,
            Self::Listening | Self::Curious => Color::Cyan,
            Self::Thinking | Self::Working => Color::LightBlue,
            Self::Speaking => Color::Magenta,
            Self::Happy => Color::Green,
            Self::Delighted | Self::Celebrating => Color::LightGreen,
            Self::Proud => Color::LightCyan,
            Self::Excited | Self::Surprised => Color::LightYellow,
            Self::Sad => Color::Blue,
            Self::Concerned => Color::Yellow,
            Self::Confused => Color::LightMagenta,
            Self::Apologetic => Color::LightBlue,
            Self::Cautious | Self::Focused => Color::Yellow,
            Self::Alarmed => Color::Red,
            Self::Worried => Color::LightRed,
            Self::Locked | Self::Sleepy => Color::DarkGray,
            Self::Waving => Color::LightGreen,
        }
    }

    /// Resolve a name as typed by a user or emitted in a directive.
    pub fn from_name(name: &str) -> Option<Mood> {
        let key: String = name
            .trim()
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_ascii_lowercase()
            .replace([' ', '-', '_'], "");
        if key.is_empty() {
            return None;
        }
        Some(match key.as_str() {
            "asleep" | "sleep" | "sleeping" => Self::Asleep,
            "sleepy" | "drowsy" => Self::Sleepy,
            "idle" | "neutral" | "calm" | "ok" => Self::Idle,
            "listening" | "listen" | "alert" | "attentive" => Self::Listening,
            "thinking" | "think" | "pondering" => Self::Thinking,
            "speaking" | "speak" | "talking" | "talk" => Self::Speaking,
            "happy" | "happiness" | "smile" => Self::Happy,
            "delighted" | "joy" | "joyful" | "cheerful" => Self::Delighted,
            "proud" | "accomplished" | "success" => Self::Proud,
            "excited" | "excite" | "eager" => Self::Excited,
            "sad" | "down" | "unhappy" => Self::Sad,
            "concerned" | "concern" | "serious" | "error" => Self::Concerned,
            "confused" | "uncertain" | "puzzled" => Self::Confused,
            "apologetic" | "sorry" | "apology" => Self::Apologetic,
            "cautious" | "careful" | "wary" => Self::Cautious,
            "alarmed" | "alarm" | "panic" | "urgent" => Self::Alarmed,
            "worried" | "worry" | "anxious" => Self::Worried,
            "locked" | "lock" | "private" => Self::Locked,
            "waving" | "wave" | "hi" | "hello" | "greeting" => Self::Waving,
            "curious" | "curiosity" => Self::Curious,
            "working" | "work" | "busy" => Self::Working,
            "focused" | "focus" => Self::Focused,
            "surprised" | "surprise" => Self::Surprised,
            "celebrating" | "celebrate" => Self::Celebrating,
            _ => return None,
        })
    }

    /// Every named mood, for `/mascot` help.
    pub fn names() -> &'static [&'static str] {
        &[
            "asleep",
            "idle",
            "listening",
            "thinking",
            "speaking",
            "happy",
            "delighted",
            "proud",
            "excited",
            "sad",
            "concerned",
            "confused",
            "sorry",
            "cautious",
            "alarm",
            "worried",
            "locked",
            "waving",
            "curious",
            "working",
            "focused",
            "surprised",
            "sleepy",
            "celebrating",
        ]
    }

    /// The mood implied purely by the current phase, before any agent override.
    pub fn automatic(s: &Snapshot) -> Mood {
        if s.locked {
            Mood::Locked
        } else if s.alarm {
            Mood::Alarmed
        } else if s.mic_error {
            Mood::Worried
        } else if s.approval {
            Mood::Cautious
        } else if s.speaking {
            Mood::Speaking
        } else if s.working {
            Mood::Working
        } else if s.thinking {
            Mood::Thinking
        } else if s.listening {
            Mood::Listening
        } else if s.active {
            Mood::Idle
        } else {
            Mood::Asleep
        }
    }
}

/// The slice of Accessor's live state that the mascot reacts to.
#[derive(Clone, Copy, Default)]
pub struct Snapshot {
    pub locked: bool,
    pub alarm: bool,
    pub mic_error: bool,
    pub speaking: bool,
    pub thinking: bool,
    pub working: bool,
    pub approval: bool,
    /// A bare wake is open and Accessor is waiting for the request.
    pub listening: bool,
    /// The conversation is open but otherwise idle.
    pub active: bool,
}

/// Remove a `[[mood:NAME]]` directive from `text`, returning the remaining text
/// and the requested mood. A directive that names an unknown mood is dropped
/// along with its marker so malformed output never leaks to the user.
pub fn take_mood(text: &str) -> (String, Option<Mood>) {
    let mut mood = None;
    let mut removed = false;
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[[mood") {
        out.push_str(&rest[..start]);
        let marker = &rest[start..];
        if let Some(close) = marker.find("]]") {
            let inner = marker[6..close].trim_start_matches([':', ' ', '=']);
            if let Some(found) = Mood::from_name(inner) {
                mood = Some(found);
            }
            removed = true;
            let mut tail = &marker[close + 2..];
            // The marker usually sat between two spaces; keep only one.
            if out.ends_with(' ') && tail.starts_with(' ') {
                tail = &tail[1..];
            }
            rest = tail;
        } else {
            // No terminator: keep the rest verbatim.
            out.push_str(marker);
            rest = "";
        }
    }
    out.push_str(rest);
    if removed {
        // A standalone marker can leave a stray space before punctuation.
        for punct in ['!', '?', '.', ',', ';', ':'] {
            out = out.replace(&format!(" {punct}"), &punct.to_string());
        }
        let trimmed = out.trim();
        if trimmed != out {
            out = trimmed.to_string();
        }
    }
    (out, mood)
}

/// A zero-latency, best-effort read of the emotional tone of a reply. Returns
/// `None` when nothing stands out, so the automatic phase mood is kept.
pub fn sense(text: &str) -> Option<Mood> {
    let t = text.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&[
        "i'm sorry",
        "i am sorry",
        "apolog",
        "my mistake",
        "my apologies",
        "i was wrong",
    ]) {
        return Some(Mood::Apologetic);
    }
    if has(&[
        "unfortunately",
        "i couldn't",
        "could not complete",
        "i was unable",
        "didn't work",
        "it failed",
        "i can't do that",
    ]) {
        return Some(Mood::Concerned);
    }
    if has(&[
        "not sure",
        "i'm not certain",
        "unclear",
        "ambiguous",
        "i don't know",
        "wasn't able to find",
    ]) {
        return Some(Mood::Confused);
    }
    if has(&[
        "be careful",
        "caution",
        "warning",
        "risky",
        "at your own risk",
        "double-check",
    ]) {
        return Some(Mood::Cautious);
    }
    if has(&["congratulations", "well done", "you're all set", "all done"]) {
        return Some(Mood::Proud);
    }
    if has(&[
        "yay",
        "awesome",
        "amazing",
        "excellent",
        "fantastic",
        "wonderful",
        "brilliant",
    ]) {
        return Some(Mood::Excited);
    }
    if has(&[
        "happy to",
        "glad to",
        "of course",
        "you're welcome",
        "my pleasure",
    ]) {
        return Some(Mood::Happy);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_and_aliases_match() {
        for name in Mood::names() {
            assert!(
                Mood::from_name(name).is_some(),
                "{name} should resolve to a mood"
            );
        }
        assert_eq!(Mood::from_name("Sorry!"), Some(Mood::Apologetic));
        assert_eq!(Mood::from_name("  listening "), Some(Mood::Listening));
        assert_eq!(Mood::from_name("banana"), None);
    }

    #[test]
    fn directive_is_removed_and_parsed() {
        let (text, mood) = take_mood("All done [[mood:proud]]!");
        assert_eq!(text, "All done!");
        assert_eq!(mood, Some(Mood::Proud));
        let (text, mood) = take_mood("Just a normal sentence.");
        assert_eq!(text, "Just a normal sentence.");
        assert_eq!(mood, None);
        let (text, mood) = take_mood("Hi [[mood:banana]] there");
        assert_eq!(text, "Hi there");
        assert_eq!(mood, None);
    }

    #[test]
    fn automatic_priority_prefers_live_phase() {
        let mut s = Snapshot {
            locked: true,
            speaking: true,
            ..Snapshot::default()
        };
        assert_eq!(Mood::automatic(&s), Mood::Locked);
        s.locked = false;
        assert_eq!(Mood::automatic(&s), Mood::Speaking);
        s.speaking = false;
        s.thinking = true;
        assert_eq!(Mood::automatic(&s), Mood::Thinking);
        s.thinking = false;
        s.listening = true;
        assert_eq!(Mood::automatic(&s), Mood::Listening);
        s.listening = false;
        s.active = true;
        assert_eq!(Mood::automatic(&s), Mood::Idle);
        s.active = false;
        assert_eq!(Mood::automatic(&s), Mood::Asleep);
        s.working = true;
        assert_eq!(Mood::automatic(&s), Mood::Working);
        s.approval = true;
        assert_eq!(Mood::automatic(&s), Mood::Cautious);
    }

    #[test]
    fn sense_picks_out_tone_or_defers() {
        assert_eq!(
            sense("I'm sorry, that didn't have to.",),
            Some(Mood::Apologetic)
        );
        assert_eq!(
            sense("Unfortunately the file was missing."),
            Some(Mood::Concerned)
        );
        assert_eq!(sense("You're all set!"), Some(Mood::Proud));
        assert_eq!(sense("Here is the total: 42."), None);
    }
}
