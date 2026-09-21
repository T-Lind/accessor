//! Local interface lock. The OS account remains the security boundary for files/CLIs.
use anyhow::{ensure, Context, Result};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    hash: String,
    failures: u32,
    retry_at: u64,
}

pub struct Lock {
    credential: Option<Credential>,
    unlocked_at: Option<Instant>,
}

/// One live interface per profile also prevents stale in-memory credentials and
/// independent guess counters. OS file locking is released automatically on exit.
pub struct Instance(std::fs::File);
impl Instance {
    pub fn acquire() -> Result<Self> {
        let directory = crate::config::home()?;
        std::fs::create_dir_all(&directory)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("accessor.lock"))?;
        fs2::FileExt::try_lock_exclusive(&file).context("Accessor is already running for this profile. Use its /password command, or close it first")?;
        Ok(Self(file))
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

pub enum Entry {
    Unlock,
    New,
    Confirm(Zeroizing<String>),
}

pub fn cli(action: &str) -> Result<()> {
    let _instance = if action == "status" {
        None
    } else {
        Some(Instance::acquire()?)
    };
    let mut lock = Lock::load()?;
    if action == "status" {
        println!(
            "Password {}. New sessions always start locked when configured.",
            if lock.enabled() {
                "configured"
            } else {
                "not configured"
            }
        );
        return Ok(());
    }
    ensure!(
        ["set", "remove"].contains(&action),
        "Use acc password set|remove|status"
    );
    // A terminal prompt avoids process arguments, shell history and echo.
    if lock.enabled() {
        let phrase = Zeroizing::new(
            dialoguer::Password::new()
                .with_prompt("Current passphrase")
                .interact()?,
        );
        ensure!(lock.verify(&phrase)?, "Incorrect passphrase");
    }
    if action == "remove" {
        lock.remove()?;
        println!("Password removed.");
    } else {
        println!("Choose four unrelated words. Case, spaces and punctuation are normalized for local spoken unlocking.");
        let phrase = Zeroizing::new(
            dialoguer::Password::new()
                .with_prompt("New passphrase")
                .with_confirmation("Repeat passphrase", "Passphrases differ")
                .interact()?,
        );
        lock.enroll(&phrase)?;
        println!("Password saved as a salted Argon2id hash. Say your wake code, unlock, then the passphrase. Restart any running Accessor session to apply this CLI change.");
    }
    Ok(())
}

/// Speech recognizers vary capitalization, spacing and punctuation. Never use
/// approximate matching: every normalized word must match, in order.
pub fn normalize(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl Lock {
    pub fn load() -> Result<Self> {
        let path = crate::config::home()?.join("password.json");
        let credential: Option<Credential> = match std::fs::read(&path) {
            Ok(bytes) => Some(
                serde_json::from_slice(&bytes)
                    .context("Invalid password file; refusing to start unlocked")?,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(e).context("Cannot read password file; refusing to start unlocked")
            }
        };
        if let Some(c) = &credential {
            PasswordHash::new(&c.hash).map_err(|_| {
                anyhow::anyhow!("Invalid password hash; refusing to start unlocked")
            })?;
        }
        Ok(Self {
            credential,
            unlocked_at: None,
        })
    }
    pub fn enabled(&self) -> bool {
        self.credential.is_some()
    }
    pub fn locked(&self) -> bool {
        self.enabled() && self.unlocked_at.is_none()
    }
    pub fn lock(&mut self) {
        self.unlocked_at = None;
    }
    pub fn expired(&self, timeout_seconds: u64, now: Instant) -> bool {
        self.unlocked_at.is_some_and(|at| {
            now.saturating_duration_since(at) >= Duration::from_secs(timeout_seconds)
        })
    }
    pub fn remaining(&self, timeout_seconds: u64) -> Option<Duration> {
        self.unlocked_at
            .map(|at| Duration::from_secs(timeout_seconds).saturating_sub(at.elapsed()))
    }
    pub fn retry_seconds(&self) -> u64 {
        self.credential
            .as_ref()
            .map_or(0, |c| c.retry_at.saturating_sub(unix_now()))
    }
    fn save(&self) -> Result<()> {
        crate::config::save_private(
            &crate::config::home()?.join("password.json"),
            &serde_json::to_vec(self.credential.as_ref().context("No password")?)?,
        )
    }
    pub fn enroll(&mut self, phrase: &str) -> Result<()> {
        ensure!(!self.locked(), "Unlock before changing the password");
        let normalized = Zeroizing::new(normalize(phrase));
        ensure!(
            normalized.len() >= 12 && normalized.split_whitespace().count() >= 3,
            "Use at least three words and 12 characters; four unrelated words are recommended"
        );
        ensure!(
            normalized.len() <= 256,
            "Passphrase is too long (maximum 256 characters)"
        );
        let salt = SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes())
            .map_err(|_| anyhow::anyhow!("Cannot create password salt"))?;
        let hash = Argon2::default()
            .hash_password(normalized.as_bytes(), &salt)
            .map_err(|_| anyhow::anyhow!("Cannot hash password"))?
            .to_string();
        let previous = self.credential.replace(Credential {
            hash,
            failures: 0,
            retry_at: 0,
        });
        if let Err(e) = self.save() {
            self.credential = previous;
            return Err(e);
        }
        self.lock();
        Ok(())
    }
    pub fn verify(&mut self, phrase: &str) -> Result<bool> {
        ensure!(
            self.retry_seconds() == 0,
            "Too many attempts. Wait {} seconds",
            self.retry_seconds()
        );
        let Some(c) = &mut self.credential else {
            return Ok(true);
        };
        let normalized = Zeroizing::new(normalize(phrase));
        let hash =
            PasswordHash::new(&c.hash).map_err(|_| anyhow::anyhow!("Invalid password hash"))?;
        let valid = normalized.len() <= 256
            && Argon2::default()
                .verify_password(normalized.as_bytes(), &hash)
                .is_ok();
        if valid {
            c.failures = 0;
            c.retry_at = 0;
        } else {
            c.failures = c.failures.saturating_add(1);
            // Persist a bounded backoff so restarting cannot reset the counter.
            c.retry_at = unix_now() + (1_u64 << c.failures.min(8)).min(300);
        }
        self.save()?;
        if valid {
            self.unlocked_at = Some(Instant::now());
        }
        Ok(valid)
    }
    pub fn remove(&mut self) -> Result<()> {
        ensure!(!self.locked(), "Unlock before removing the password");
        if self.enabled() {
            std::fs::remove_file(crate::config::home()?.join("password.json"))?;
        }
        self.credential = None;
        self.unlocked_at = None;
        Ok(())
    }
}

/// Always consume explicit unlock attempts locally, even when already unlocked.
/// This prevents a repeated passphrase from becoming an agent prompt.
pub fn spoken_unlock<'a>(wake: &crate::wake::WakeCode, text: &'a str) -> Option<&'a str> {
    let rest = wake.strip(text)?.trim();
    let word = rest.get(..6)?;
    if !word.eq_ignore_ascii_case("unlock") {
        return None;
    }
    let phrase = rest.get(6..)?;
    (phrase.is_empty() || phrase.starts_with(|c: char| !c.is_alphanumeric()))
        .then(|| phrase.trim_start_matches(|c: char| !c.is_alphanumeric()))
}

pub fn spoken_lock(wake: &crate::wake::WakeCode, text: &str) -> bool {
    wake.strip(text).is_some_and(|s| normalize(s) == "lock")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_words_only_and_wake_required() {
        let wake = crate::wake::WakeCode::new("29", &[]).unwrap();
        assert_eq!(
            spoken_unlock(&wake, "twenty nine unlock Blue, River Lantern!"),
            Some("Blue, River Lantern!")
        );
        assert!(spoken_unlock(&wake, "unlock blue river lantern").is_none());
        assert!(spoken_unlock(&wake, "29 unlocking secrets").is_none());
        assert!(spoken_lock(&wake, "29 lock!"));
        assert!(!spoken_lock(&wake, "29 lock the door"));
        assert_eq!(normalize("Blue, RIVER-lantern!"), "blue river lantern");
        assert_ne!(
            normalize("blue river lantern"),
            normalize("blue river lantern send email")
        );
    }
    #[test]
    fn expiry_is_absolute_and_password_starts_locked() {
        let now = Instant::now();
        let mut lock = Lock {
            credential: Some(Credential {
                hash: String::new(),
                failures: 0,
                retry_at: 0,
            }),
            unlocked_at: None,
        };
        assert!(lock.locked());
        lock.unlocked_at = Some(now);
        assert!(!lock.expired(3600, now + Duration::from_secs(3599)));
        assert!(lock.expired(3600, now + Duration::from_secs(3600)));
        lock.lock();
        assert!(lock.locked());
    }
}
