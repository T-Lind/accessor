//! Exact activation matching, intentionally independent of the agent.
use anyhow::{ensure, Result};

#[derive(Debug)]
pub struct WakeCode {
    aliases: Vec<Vec<String>>,
}

fn tokens(text: &str) -> Vec<(usize, usize, String)> {
    let mut result = Vec::new();
    let mut start = None;
    for (i, ch) in text.char_indices() {
        if ch.is_alphanumeric() {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            result.push((s, i, text[s..i].to_lowercase()));
        }
    }
    if let Some(s) = start {
        result.push((s, text.len(), text[s..].to_lowercase()));
    }
    result
}

impl WakeCode {
    pub fn new(code: &str, aliases: &[String]) -> Result<Self> {
        ensure!(
            !code.is_empty() && code.len() <= 12 && code.bytes().all(|c| c.is_ascii_digit()),
            "wake code must contain 1–12 digits"
        );
        let digits = [
            "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
        ];
        let mut forms = vec![
            code.to_owned(),
            code.bytes()
                .map(|c| digits[(c - b'0') as usize])
                .collect::<Vec<_>>()
                .join(" "),
        ];
        if code.len() <= 2 && !code.starts_with('0') {
            let n: usize = code.parse()?;
            let teens = [
                "ten",
                "eleven",
                "twelve",
                "thirteen",
                "fourteen",
                "fifteen",
                "sixteen",
                "seventeen",
                "eighteen",
                "nineteen",
            ];
            let tens = [
                "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty",
                "ninety",
            ];
            if (10..20).contains(&n) {
                forms.push(teens[n - 10].into());
            } else if n >= 20 {
                forms.push(format!(
                    "{} {}",
                    tens[n / 10],
                    if n.is_multiple_of(10) {
                        ""
                    } else {
                        digits[n % 10]
                    }
                ));
            }
        }
        forms.extend_from_slice(aliases);
        let mut aliases: Vec<Vec<String>> = forms
            .iter()
            .map(|s| tokens(s).into_iter().map(|(_, _, t)| t).collect())
            .collect();
        ensure!(
            aliases.iter().all(|v| !v.is_empty()),
            "wake aliases cannot be empty"
        );
        aliases.sort_by_key(|v| std::cmp::Reverse(v.len()));
        Ok(Self { aliases })
    }
    /// Returns only the text after an exact prefix. Never searches mid-sentence.
    /// Optional leading attention words ("hey", "hi", "ok", "okay") are allowed.
    pub fn strip<'a>(&self, text: &'a str) -> Option<&'a str> {
        let words = tokens(text);
        let mut offsets = vec![0];
        if words
            .first()
            .is_some_and(|w| ["hey", "hi", "ok", "okay"].contains(&w.2.as_str()))
        {
            offsets.push(1);
        }
        for skip in offsets {
            if let Some(rest) = self.strip_at(text, &words, skip) {
                return Some(rest);
            }
        }
        None
    }
    fn strip_at<'a>(
        &self,
        text: &'a str,
        words: &[(usize, usize, String)],
        skip: usize,
    ) -> Option<&'a str> {
        for alias in &self.aliases {
            if words.len() < skip + alias.len() {
                continue;
            }
            if !words[skip..].iter().zip(alias).all(|(w, a)| &w.2 == a) {
                continue;
            }
            let end = words[skip + alias.len() - 1].1;
            // Avoid recognizing a prefix of a longer spoken or formatted number.
            if let Some(next) = words.get(skip + alias.len()) {
                if next.2.bytes().all(|b| b.is_ascii_digit())
                    || [
                        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight",
                        "nine", "hundred", "thousand",
                    ]
                    .contains(&next.2.as_str())
                {
                    continue;
                }
            }
            return Some(text[end..].trim_start_matches(|c: char| !c.is_alphanumeric()));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deliberate_prefixes() {
        let w = WakeCode::new("29", &[]).unwrap();
        for s in [
            "29, List files.",
            "Twenty-nine, List files.",
            "two nine List files.",
        ] {
            assert_eq!(w.strip(s), Some("List files."));
        }
        for s in [
            "129 list files",
            "We have 29 files",
            "twenty nine hundred files",
            "29.5 degrees",
            "twenty ninety",
            "hey list files",
        ] {
            assert_eq!(w.strip(s), None, "{s}");
        }
        for s in [
            "Hey 29, List files.",
            "okay twenty-nine List files.",
            "hi two nine List files.",
            "OK 29 List files.",
        ] {
            assert_eq!(w.strip(s), Some("List files."), "{s}");
        }
        assert_eq!(w.strip("Hey 29"), Some(""));
    }
    #[test]
    fn leading_zero_and_alias() {
        let w = WakeCode::new("029", &["Accessor twenty nine".into()]).unwrap();
        assert_eq!(w.strip("zero two nine hello"), Some("hello"));
        assert_eq!(w.strip("29 hello"), None);
        assert_eq!(w.strip("Accessor twenty nine hello"), Some("hello"));
    }
}
