use crate::wake::WakeCode;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
pub enum Action {
    Ignore,
    Open,
    Prompt(String),
    Mute,
    Unmute,
    Disconnect,
    Cancel,
    Stop,
}

pub struct Session {
    wake: WakeCode,
    active: bool,
    addressed: bool,
    last: Instant,
    timeout: Duration,
}
impl Session {
    pub fn new(wake: WakeCode, addressed: bool, timeout: Duration) -> Self {
        Self {
            wake,
            active: false,
            addressed,
            last: Instant::now(),
            timeout,
        }
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn addressed(&self, text: &str) -> bool {
        self.wake.strip(text).is_some()
    }
    pub fn close(&mut self) {
        self.active = false;
    }
    pub fn touch(&mut self, now: Instant) {
        if self.active {
            self.last = now;
        }
    }
    pub fn expire(&mut self, now: Instant) -> bool {
        if self.active && !self.timeout.is_zero() && now.duration_since(self.last) >= self.timeout {
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
        let control = text
            .trim_matches(|c: char| c.is_ascii_punctuation())
            .to_lowercase();
        if is_sleep(&control) {
            self.close();
            return Action::Disconnect;
        }
        match control.as_str() {
            "mute" => {
                self.close();
                return Action::Mute;
            }
            "unmute" => {
                self.close();
                return Action::Unmute;
            }
            "stop" => {
                self.close();
                return Action::Stop;
            }
            "cancel the task" | "cancel task" => return Action::Cancel,
            _ => {}
        }
        if text.is_empty() {
            self.active = true;
            Action::Open
        } else {
            self.active = !self.addressed;
            Action::Prompt(text.into())
        }
    }

    /// While privacy-muted, keep the wake detector local and accept only the
    /// exact unmute control. No other phrase may open a session.
    pub fn hear_while_muted(&mut self, text: &str, now: Instant) -> Action {
        self.close();
        let action = self.hear(text, now);
        self.close();
        if matches!(action, Action::Unmute) {
            action
        } else {
            Action::Ignore
        }
    }
}

fn is_sleep(text: &str) -> bool {
    matches!(
        text,
        "disconnect" | "sleep" | "go to sleep" | "go back to sleep" | "good night" | "goodnight"
    ) || text.ends_with(" go to sleep")
        || text.ends_with(" go back to sleep")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stop_sleep_and_disabled_timeout() {
        let now = Instant::now();
        let mut s = Session::new(WakeCode::new("29", &[]).unwrap(), false, Duration::ZERO);
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
    }
    #[test]
    fn activity_postpones_sleep_without_reopening_it() {
        let now = Instant::now();
        let mut s = Session::new(
            WakeCode::new("29", &[]).unwrap(),
            false,
            Duration::from_secs(2),
        );
        s.touch(now);
        assert!(!s.active());
        s.hear("29", now);
        s.touch(now + Duration::from_secs(10));
        assert!(!s.expire(now + Duration::from_secs(11)));
        assert!(s.expire(now + Duration::from_secs(12)));
    }
    #[test]
    fn timeout_and_followups() {
        let now = Instant::now();
        let mut s = Session::new(
            WakeCode::new("29", &[]).unwrap(),
            false,
            Duration::from_secs(30),
        );
        assert_eq!(s.hear("hello", now), Action::Ignore);
        assert_eq!(s.hear("29", now), Action::Open);
        assert_eq!(s.hear("hello", now), Action::Prompt("hello".into()));
        assert_eq!(
            s.hear("hello", now + Duration::from_secs(31)),
            Action::Ignore
        );
    }
    #[test]
    fn addressed_and_disconnect() {
        let now = Instant::now();
        let mut s = Session::new(
            WakeCode::new("29", &[]).unwrap(),
            true,
            Duration::from_secs(30),
        );
        assert_eq!(s.hear("29 hello", now), Action::Prompt("hello".into()));
        assert!(!s.active());
        assert_eq!(s.hear("hello", now), Action::Ignore);
        assert_eq!(s.hear("29", now), Action::Open);
        assert!(s.active());
        assert_eq!(s.hear("followup", now), Action::Prompt("followup".into()));
        assert!(!s.active());
        assert_eq!(s.hear("29 disconnect", now), Action::Disconnect);
        assert_eq!(s.hear("cancel the task", now), Action::Ignore);
        assert!(s.addressed("29 help"));
        assert!(!s.addressed("help"));
    }
    #[test]
    fn go_back_to_sleep_and_hey_wake() {
        let now = Instant::now();
        let mut s = Session::new(
            WakeCode::new("29", &[]).unwrap(),
            false,
            Duration::from_secs(30),
        );
        assert_eq!(s.hear("Hey 29, hello", now), Action::Prompt("hello".into()));
        assert!(s.active());
        assert_eq!(s.hear("please go back to sleep", now), Action::Disconnect);
        assert!(!s.active());
        assert_eq!(s.hear("still talking", now), Action::Ignore);
        assert_eq!(s.hear("hey 29", now), Action::Open);
    }

    #[test]
    fn mute_controls_are_exact_and_local() {
        let now = Instant::now();
        let mut s = Session::new(
            WakeCode::new("29", &[]).unwrap(),
            false,
            Duration::from_secs(30),
        );
        assert_eq!(s.hear("29 mute", now), Action::Mute);
        assert!(!s.active());
        assert_eq!(s.hear("unmute", now), Action::Ignore);
        assert_eq!(s.hear("29 unmute", now), Action::Unmute);
        assert!(!s.active());
        assert_eq!(
            s.hear("29 mute the television", now),
            Action::Prompt("mute the television".into())
        );

        assert_eq!(s.hear_while_muted("29 hello", now), Action::Ignore);
        assert!(!s.active());
        assert_eq!(s.hear_while_muted("unmute", now), Action::Ignore);
        assert_eq!(s.hear_while_muted("29 unmute", now), Action::Unmute);
        assert!(!s.active());
    }
}
