//! Local-only private journal. Dictation appends here and the text never leaves
//! this computer: it is never sent to an agent or a cloud service. Entries are
//! encrypted at rest with XChaCha20-Poly1305, and the key lives in the OS
//! credential store (or the `JOURNAL_KEY` environment variable, base64).
use crate::config;
use anyhow::{anyhow, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    XChaCha20Poly1305, XNonce,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const NONCE_LEN: usize = 24;
const MAX_BYTES: u64 = 8_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub at: u64,
    pub text: String,
}

pub fn path() -> Result<PathBuf> {
    Ok(config::home()?.join("journal").join("journal.enc"))
}

fn key() -> Result<[u8; 32]> {
    if let Some(encoded) = config::optional_secret("journal", "JOURNAL_KEY") {
        let bytes = STANDARD
            .decode(encoded.trim())
            .context("JOURNAL_KEY must be base64")?;
        ensure!(bytes.len() == 32, "JOURNAL_KEY must decode to 32 bytes");
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        return Ok(key);
    }
    let generated = XChaCha20Poly1305::generate_key(&mut OsRng);
    config::save_secret("journal", &STANDARD.encode(generated))?;
    let mut key = [0u8; 32];
    key.copy_from_slice(&generated);
    Ok(key)
}

fn read_entries() -> Result<Vec<Entry>> {
    let path = path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    ensure!(
        std::fs::metadata(&path)?.len() <= MAX_BYTES,
        "Journal file exceeds the size limit"
    );
    let raw = std::fs::read(&path)?;
    ensure!(raw.len() >= NONCE_LEN, "Journal file is malformed");
    let (nonce, ciphertext) = raw.split_at(NONCE_LEN);
    let cipher =
        XChaCha20Poly1305::new_from_slice(&key()?).map_err(|_| anyhow!("Invalid journal key"))?;
    let plaintext = cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| anyhow!("Journal could not be decrypted with the stored key"))?;
    let text = String::from_utf8(plaintext).context("Journal is not valid UTF-8")?;
    let mut entries = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        entries.push(serde_json::from_str(line).context("Journal entry is malformed")?);
    }
    Ok(entries)
}

fn write_entries(entries: &[Entry]) -> Result<()> {
    let mut text = String::new();
    for entry in entries {
        text.push_str(&serde_json::to_string(entry)?);
        text.push('\n');
    }
    let cipher =
        XChaCha20Poly1305::new_from_slice(&key()?).map_err(|_| anyhow!("Invalid journal key"))?;
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, text.as_bytes())
        .map_err(|_| anyhow!("Journal encryption failed"))?;
    let mut raw = nonce.to_vec();
    raw.extend_from_slice(&ciphertext);
    let path = path()?;
    std::fs::create_dir_all(path.parent().context("Journal parent missing")?)?;
    config::save_private(&path, &raw)
}

/// Append one locally transcribed utterance. Returns the new entry count.
pub fn append(text: &str) -> Result<usize> {
    let text = text.trim();
    ensure!(
        !text.is_empty() && text.len() <= 8000,
        "Journal entry must be 1–8000 bytes"
    );
    let mut entries = read_entries()?;
    entries.push(Entry {
        at: now_unix(),
        text: text.to_string(),
    });
    write_entries(&entries)?;
    Ok(entries.len())
}

pub fn render() -> Result<String> {
    let entries = read_entries()?;
    if entries.is_empty() {
        return Ok(
            "Journal is empty. Local-only dictation writes here and never leaves this computer."
                .into(),
        );
    }
    let mut out = format!(
        "Private journal — {} entries, encrypted at rest\n\n",
        entries.len()
    );
    for entry in &entries {
        let stamp = chrono::DateTime::from_timestamp(entry.at as i64, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| entry.at.to_string());
        out.push_str(&format!("{stamp}\n{}\n\n", entry.text));
    }
    Ok(out)
}

pub fn clear() -> Result<()> {
    let path = path()?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_round_trips_through_the_codec() {
        let entry = Entry {
            at: 1_700_000_000,
            text: "A private thought".into(),
        };
        let line = serde_json::to_string(&entry).unwrap();
        let back: Entry = serde_json::from_str(&line).unwrap();
        assert_eq!(back.text, entry.text);
        assert_eq!(back.at, entry.at);
    }

    #[test]
    fn key_is_32_bytes_when_generated() {
        let generated = XChaCha20Poly1305::generate_key(&mut OsRng);
        assert_eq!(generated.len(), 32);
    }

    #[test]
    fn codec_round_trips_with_a_fixed_key() {
        let key = [7u8; 32];
        let cipher = XChaCha20Poly1305::new_from_slice(&key).unwrap();
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher.encrypt(&nonce, b"private entry".as_slice()).unwrap();
        let mut raw = nonce.to_vec();
        raw.extend_from_slice(&ciphertext);
        let (stored_nonce, stored) = raw.split_at(NONCE_LEN);
        let plaintext = cipher
            .decrypt(XNonce::from_slice(stored_nonce), stored)
            .unwrap();
        assert_eq!(plaintext, b"private entry");
    }
}
