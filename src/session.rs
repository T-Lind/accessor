use crate::wake::WakeCode;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
pub enum Action {
    Ignore,
    Open,
    Prompt(String),
    Disconnect,
    Cancel,
    Stop,
}

pub struct Session {
    wake: WakeCode,
    active: bool,
    last: Instant,
    timeout: Duration,
    grace_until: Option<Instant>,
}
impl Session {
    pub fn new(wake: WakeCode, timeout: Duration) -> Self {
        Self {
            wake,
            active: false,
            last: Instant::now(),
            timeout,
            grace_until: None,
        }
    }
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn wake_in_probe(&self, text: &str) -> bool {
        self.wake.in_probe(text)
    }
    pub fn addressed(&self, text: &str) -> bool {
        self.wake.strip(text).is_some()
    }
    pub fn replaces_work(&self, text: &str) -> bool {
        stop_replacement(self.wake.strip(text).unwrap_or(text)).is_some()
    }
    pub fn close(&mut self) {
        self.active = false;
        self.grace_until = None;
    }
    pub fn touch(&mut self, now: Instant) {
        if self.active {
            self.last = now;
        }
    }
    pub fn expire(&mut self, now: Instant) -> bool {
        if self.active
            && self.grace_until.is_none_or(|until| now >= until)
            && !self.timeout.is_zero()
            && now.duration_since(self.last) >= self.timeout
        {
            self.close();
            true
        } else {
            false
        }
    }
    pub fn hear(&mut self, text: &str, now: Instant) -> Action {
        self.expire(now);
        let prefix = self.wake.strip(text);
        if !self.active && prefix.is_none() {
            return Action::Ignore;
        }
        let text = prefix.unwrap_or(text).trim();
        self.last = now;
        if is_sleep(text) {
            self.close();
            return Action::Disconnect;
        }
        let control = text
            .trim_matches(|c: char| c.is_ascii_punctuation())
            .to_lowercase();
        match control.as_str() {
            "stop" => {
                self.close();
                return Action::Stop;
            }
            "cancel the task" | "cancel task" => return Action::Cancel,
            _ => {}
        }
        if let Some(replacement) = stop_replacement(text) {
            self.active = true;
            return Action::Prompt(replacement.into());
        }
        if text.is_empty() {
            self.active = true;
            self.grace_until = Some(now + Duration::from_secs(8));
            Action::Open
        } else {
            self.active = true;
            Action::Prompt(text.into())
        }
    }
}

fn stop_replacement(text: &str) -> Option<&str> {
    let text = text.trim_start();
    let rest = text.get(4..)?;
    if !text.get(..4)?.eq_ignore_ascii_case("stop") || !rest.starts_with([',', '.', ';', ':', '!'])
    {
        return None;
    }
    let rest = rest.trim_start_matches(|c: char| !c.is_alphanumeric());
    (!rest.is_empty()).then_some(rest)
}

fn is_sleep(text: &str) -> bool {
    let clean = text.trim().to_lowercase();
    let words: Vec<&str> = clean
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .collect();

    if words.is_empty() {
        return false;
    }

    if matches!(
        clean.trim_matches(|c: char| !c.is_alphabetic()),
        "disconnect"
            | "sleep"
            | "go to sleep"
            | "go back to sleep"
            | "good night"
            | "goodnight"
            | "stop listening"
            | "go to bed"
            | "turn off"
            | "turn yourself off"
            | "shut down"
            | "shut off"
            | "power down"
            | "be quiet"
            | "shut up"
    ) {
        return true;
    }

    // Do not treat as sleep if it looks like a question or coding instruction
    if words.iter().any(|w| {
        matches!(
            *w,
            "how"
                | "why"
                | "what"
                | "write"
                | "explain"
                | "code"
                | "function"
                | "script"
                | "thread"
        )
    }) {
        return false;
    }

    let sleep_patterns: &[&[&str]] = &[
        &["go", "to", "sleep"],
        &["go", "back", "to", "sleep"],
        &["stop", "listening"],
        &["go", "to", "bed"],
        &["disconnect"],
        &["good", "night"],
        &["goodnight"],
        &["turn", "off"],
        &["turn", "yourself", "off"],
        &["shut", "down"],
        &["shut", "off"],
        &["power", "down"],
        &["be", "quiet"],
        &["shut", "up"],
    ];

    let allowed_trailing: &[&str] = &[
        "now", "please", "then", "thanks", "thank", "you", "ok", "okay", "for", "bye", "goodbye",
        "goodnight", "night",
    ];

    for pattern in sleep_patterns {
        if let Some(pos) = words.windows(pattern.len()).position(|window| window == *pattern) {
            let after = &words[pos + pattern.len()..];
            if after.iter().all(|w| allowed_trailing.contains(w)) {
                return true;
            }
        }
    }

    if let Some(pos) = words.iter().position(|w| *w == "sleep") {
        let before = &words[..pos];
        let after = &words[pos + 1..];
        let allowed_before = &["please", "you", "can", "now", "just", "time", "to", "and"];
        if before.iter().all(|w| allowed_before.contains(w))
            && after.iter().all(|w| allowed_trailing.contains(w))
        {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_requests_reach_the_agent_intact() {
        let now = Instant::now();
        let mut s = Session::new(WakeCode::new("29", &[]).unwrap(), Duration::from_secs(120));
        assert_eq!(s.hear("stop the alarm", now), Action::Ignore);
        for text in [
            "stop the alarm",
            "never mind",
            "never mind that, stop the alarm",
            "stop playing music",
        ] {
            assert_eq!(
                s.hear(&format!("29 {text}"), now),
                Action::Prompt(text.into())
            );
            assert!(!s.replaces_work(text));
        }
    }
    #[test]
    fn bare_wake_allows_time_to_begin_speaking() {
        let now = Instant::now();
        let mut s = Session::new(WakeCode::new("29", &[]).unwrap(), Duration::from_secs(1));
        assert_eq!(s.hear("29", now), Action::Open);
        assert!(!s.expire(now + Duration::from_secs(7)));
        assert!(s.expire(now + Duration::from_secs(8)));
    }
    #[test]
    fn stop_sleep_and_disabled_timeout() {
        let now = Instant::now();
        let mut s = Session::new(WakeCode::new("29", &[]).unwrap(), Duration::ZERO);
        assert_eq!(s.hear("29", now), Action::Open);
        assert!(!s.expire(now + Duration::from_secs(7200)));
        assert_eq!(
            s.hear("29 stop", now + Duration::from_secs(7201)),
            Action::Stop
        );
        assert!(!s.active());
        assert_eq!(
            s.hear("private", now + Duration::from_secs(7202)),
            Action::Ignore
        );
        assert_eq!(
            s.hear("29 stop. Please tell me about Mars", now),
            Action::Prompt("Please tell me about Mars".into())
        );
    }
    #[test]
    fn activity_postpones_sleep_without_reopening_it() {
        let now = Instant::now();
        let mut s = Session::new(WakeCode::new("29", &[]).unwrap(), Duration::from_secs(2));
        s.touch(now);
        assert!(!s.active());
        s.hear("29", now);
        s.touch(now + Duration::from_secs(10));
        assert!(!s.expire(now + Duration::from_secs(11)));
        assert!(s.expire(now + Duration::from_secs(12)));
    }
    #[test]
    fn bare_wake_opens_followup_conversation() {
        let now = Instant::now();
        let mut s = Session::new(WakeCode::new("29", &[]).unwrap(), Duration::from_secs(30));
        assert_eq!(s.hear("hello", now), Action::Ignore);
        assert_eq!(s.hear("29", now), Action::Open);
        assert_eq!(s.hear("hello", now), Action::Prompt("hello".into()));
        assert!(s.active());
        assert_eq!(s.hear("hello", now), Action::Prompt("hello".into()));
    }
    #[test]
    fn addressed_and_disconnect() {
        let now = Instant::now();
        let mut s = Session::new(WakeCode::new("29", &[]).unwrap(), Duration::from_secs(30));
        assert_eq!(s.hear("29 hello", now), Action::Prompt("hello".into()));
        assert!(s.active());
        assert_eq!(s.hear("hello", now), Action::Prompt("hello".into()));
        assert_eq!(s.hear("29", now), Action::Open);
        assert!(s.active());
        assert_eq!(s.hear("followup", now), Action::Prompt("followup".into()));
        assert!(s.active());
        assert_eq!(s.hear("29 disconnect", now), Action::Disconnect);
        assert_eq!(s.hear("cancel the task", now), Action::Ignore);
        assert!(s.addressed("29 help"));
        assert!(!s.addressed("help"));
    }
    #[test]
    fn go_back_to_sleep_and_hey_wake() {
        let now = Instant::now();
        let mut s = Session::new(WakeCode::new("29", &[]).unwrap(), Duration::from_secs(30));
        assert_eq!(s.hear("Hey 29, hello", now), Action::Prompt("hello".into()));
        assert!(s.active());
        assert_eq!(s.hear("please go back to sleep", now), Action::Disconnect);
        assert_eq!(s.hear("29 go back to sleep", now), Action::Disconnect);
        assert!(!s.active());
        assert_eq!(s.hear("still talking", now), Action::Ignore);
        assert_eq!(s.hear("hey 29", now), Action::Open);
    }

    #[test]
    fn mute_words_are_normal_agent_requests_not_a_second_state() {
        let now = Instant::now();
        let mut s = Session::new(WakeCode::new("29", &[]).unwrap(), Duration::from_secs(120));
        assert_eq!(
            s.hear("29 please mute", now),
            Action::Prompt("please mute".into())
        );
        assert!(s.active());
        assert_eq!(s.hear("unmute", now), Action::Prompt("unmute".into()));
        assert_eq!(s.hear("go to sleep", now), Action::Disconnect);
        assert_eq!(s.hear("29", now), Action::Open);
    }

    #[test]
    fn conversational_sleep_commands_disconnect() {
        let now = Instant::now();
        let mut s = Session::new(WakeCode::new("29", &[]).unwrap(), Duration::from_secs(120));
        assert_eq!(s.hear("29", now), Action::Open);
        assert!(s.active());
        assert_eq!(
            s.hear("just um, that's fine, go to sleep", now),
            Action::Disconnect
        );
        assert!(!s.active());

        assert_eq!(s.hear("29", now), Action::Open);
        assert_eq!(
            s.hear("that's all, go to sleep now", now),
            Action::Disconnect
        );

        assert_eq!(s.hear("29", now), Action::Open);
        assert_eq!(s.hear("stop listening", now), Action::Disconnect);

        assert_eq!(s.hear("29", now), Action::Open);
        assert_eq!(s.hear("turn off please", now), Action::Disconnect);

        assert_eq!(s.hear("29", now), Action::Open);
        assert_eq!(s.hear("turn yourself off now", now), Action::Disconnect);

        assert_eq!(s.hear("29", now), Action::Open);
        assert_eq!(s.hear("shut down", now), Action::Disconnect);

        assert_eq!(s.hear("29", now), Action::Open);
        assert_eq!(s.hear("be quiet", now), Action::Disconnect);

        assert_eq!(s.hear("29", now), Action::Open);
        assert_eq!(
            s.hear("turn off the kitchen lights", now),
            Action::Prompt("turn off the kitchen lights".into())
        );

        assert_eq!(s.hear("29", now), Action::Open);
        assert_eq!(
            s.hear("shut down the postgres container", now),
            Action::Prompt("shut down the postgres container".into())
        );

        assert_eq!(s.hear("29", now), Action::Open);
        assert_eq!(
            s.hear("how do I make a thread go to sleep in rust?", now),
            Action::Prompt("how do I make a thread go to sleep in rust?".into())
        );
    }
}
