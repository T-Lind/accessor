//! Bramble, the full-size animated honey-bear companion, whose expression
//! mirrors Accessor's state.
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

/// The companion's name. Keeping it in one place stops the title bar, help
/// text, and documentation from drifting apart.
pub const NAME: &str = "Bramble";

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
            Self::Listening => ["  .----.  ", " ( O  O ) ", "  ' .. '  "],
            Self::Curious => ["  .----.  ", " ( o  O ) ", "  ' ~  ' ?"],
            Self::Thinking => ["  .----.  ", " ( o  o ) ", "  ' ~~ ' ."],
            Self::Working => ["  .----.  ", " ( o  o ) ", "  ' [] ' ."],
            Self::Speaking => ["  .----.  ", " ( O  O ) ", "  ' \\/ '  "],
            Self::Happy => ["  .----.  ", " ( ^  ^ ) ", "  ' \\/ '  "],
            Self::Delighted | Self::Celebrating => ["  .----.  ", " ( ^  ^ ) ", "  ' \\/ '* "],
            Self::Proud => ["  .----.  ", " ( ^  ^ ) ", "  ' \\/ ' +"],
            Self::Excited => ["  .----.  ", " ( ^  ^ ) ", "  ' \\/ ' *"],
            Self::Surprised => ["  .----.  ", " ( ^  o ) ", "  ' \\/ ' !"],
            Self::Sad => ["  .----.  ", " ( ;  ; ) ", "  ' /\\ '  "],
            Self::Concerned => ["  .----.  ", " ( o  o ) ", "  ' /\\ '  "],
            Self::Confused => ["  .----.  ", " ( o  O ) ", "  ' ~  ' ?"],
            Self::Apologetic => ["  .----.  ", " ( >  < ) ", "  ' /\\ ' '"],
            Self::Cautious => ["  .----.  ", " ( o  o ) ", "  ' -- ' #"],
            Self::Focused => ["  .----.  ", " (= o o =)", "  ' -- '  "],
            Self::Sleepy => ["  .----.  ", " ( -  - ) ", "  ' .. ' z"],
            Self::Alarmed => ["  .----.  ", " ( O  O ) ", "  ' /\\ '!!"],
            Self::Worried => ["  .----.  ", " ( o  o ) ", "  ' ~  '  "],
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
            Self::Sad => Color::LightBlue,
            Self::Concerned => Color::Yellow,
            Self::Confused => Color::LightMagenta,
            Self::Apologetic => Color::LightBlue,
            Self::Cautious | Self::Focused => Color::Yellow,
            Self::Alarmed => Color::Red,
            Self::Worried => Color::LightRed,
            Self::Locked => Color::LightYellow,
            Self::Sleepy => Color::Gray,
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
        } else if s.approval {
            Mood::Cautious
        } else if s.mic_error {
            Mood::Worried
        } else if s.speaking {
            Mood::Speaking
        } else if s.working {
            Mood::Working
        } else if s.thinking || s.processing {
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
    /// A wake is open and Accessor is waiting for the request.
    pub listening: bool,
    /// Speech is being transcribed or checked for relevance.
    pub processing: bool,
    /// The conversation is open but otherwise idle.
    pub active: bool,
}

/// Remove a `[[mood:NAME]]` directive from `text`, returning the remaining text
/// and the requested mood. A directive that names an unknown mood is dropped
/// along with its marker so malformed output never leaks to the user. Matching
/// is case-insensitive and requires a real directive separator, so prose such
/// as `[[moodboard]]` is left alone. When several directives are present the
/// last one wins. An unterminated marker is dropped with the rest of the text.
pub fn take_mood(text: &str) -> (String, Option<Mood>) {
    let lower = text.to_ascii_lowercase();
    let mut mood = None;
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(rel) = lower[cursor..].find("[[mood") {
        let start = cursor + rel;
        let after = start + 6;
        let valid = match text[after..].chars().next() {
            Some(':') | Some('=') | Some(']') => true,
            Some(c) if c.is_whitespace() => true,
            _ => false,
        };
        if !valid {
            // Not a directive, e.g. "[[moodboard]]": keep the text.
            out.push_str(&text[cursor..after]);
            cursor = after;
            continue;
        }
        out.push_str(&text[cursor..start]);
        let Some(close) = text[after..].find("]]") else {
            // A truncated marker is machine output, not something the user
            // should hear; drop it and everything after it.
            if out.ends_with(' ') {
                out.pop();
            }
            cursor = text.len();
            break;
        };
        let close = after + close;
        let inner = text[after..close].trim_start_matches([':', ' ', '=']);
        if let Some(found) = Mood::from_name(inner) {
            mood = Some(found);
        }
        cursor = close + 2;
        if out.ends_with(' ') {
            match text[cursor..].chars().next() {
                // The marker sat between two spaces; keep only one.
                Some(' ') => cursor += 1,
                // A standalone marker leaves " !" behind; close the gap.
                Some('!' | '?' | '.' | ',' | ';' | ':') => {
                    out.pop();
                }
                _ => {}
            }
        }
    }
    out.push_str(&text[cursor..]);
    (out, mood)
}

/// A zero-latency, best-effort read of the emotional tone of a reply. Returns
/// `None` when nothing stands out, so the automatic phase mood is kept.
/// Matching uses whole words so "forewarning" does not read as "warning".
pub fn sense(text: &str) -> Option<Mood> {
    let text = text.to_lowercase().replace(['’', '‘'], "'");
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|word| !word.is_empty())
        .collect();
    let has = |phrase: &str| {
        let phrase: Vec<&str> = phrase.split_whitespace().collect();
        !phrase.is_empty()
            && words
                .windows(phrase.len())
                .any(|window| window == phrase.as_slice())
    };
    let any = |phrases: &[&str]| phrases.iter().any(|phrase| has(phrase));
    if any(&[
        "i'm sorry",
        "i am sorry",
        "apologize",
        "apologies",
        "apologetic",
        "my mistake",
        "my apologies",
        "i was wrong",
    ]) {
        return Some(Mood::Apologetic);
    }
    if any(&[
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
    if any(&[
        "not sure",
        "i'm not certain",
        "unclear",
        "ambiguous",
        "i don't know",
        "wasn't able to find",
    ]) {
        return Some(Mood::Confused);
    }
    if any(&[
        "be careful",
        "caution",
        "warning",
        "risky",
        "at your own risk",
        "double-check",
    ]) {
        return Some(Mood::Cautious);
    }
    if any(&["congratulations", "well done", "you're all set", "all done"]) {
        return Some(Mood::Proud);
    }
    if any(&[
        "yay",
        "awesome",
        "amazing",
        "fantastic",
        "wonderful",
        "brilliant",
    ]) {
        return Some(Mood::Excited);
    }
    if any(&[
        "happy to",
        "glad to",
        "of course",
        "excellent",
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
        let (text, mood) = take_mood("Hi [[MOOD:Happy]] there");
        assert_eq!(text, "Hi there");
        assert_eq!(mood, Some(Mood::Happy));
        let (text, mood) = take_mood("A [[moodboard]] is not a directive.");
        assert_eq!(text, "A [[moodboard]] is not a directive.");
        assert_eq!(mood, None);
        let (text, mood) = take_mood("One [[mood:sad]] two [[mood:proud]] three");
        assert_eq!(text, "One two three");
        assert_eq!(mood, Some(Mood::Proud));
        let (text, mood) = take_mood("Broken [[mood:ha");
        assert_eq!(text, "Broken");
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
        s.working = false;
        s.approval = true;
        assert_eq!(Mood::automatic(&s), Mood::Cautious);
        s.approval = false;
        s.mic_error = true;
        assert_eq!(Mood::automatic(&s), Mood::Worried);
        s.mic_error = false;
        s.processing = true;
        assert_eq!(Mood::automatic(&s), Mood::Thinking);
    }

    #[test]
    fn sense_picks_out_tone_or_defers() {
        assert_eq!(
            sense("I'm sorry, that didn't have to.",),
            Some(Mood::Apologetic)
        );
        assert_eq!(sense("I’m sorry about that."), Some(Mood::Apologetic));
        assert_eq!(
            sense("Unfortunately the file was missing."),
            Some(Mood::Concerned)
        );
        assert_eq!(sense("You're all set!"), Some(Mood::Proud));
        assert_eq!(sense("Here is the total: 42."), None);
        assert_eq!(sense("That's an excellent question."), Some(Mood::Happy));
        assert_eq!(sense("A forewarning about the build."), None);
        assert_eq!(sense("The test failed."), None);
    }
}
