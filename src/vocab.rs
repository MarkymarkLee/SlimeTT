//! User vocabulary: "heard → write" replacements, learned from edits in the popup or added in Settings.
//! Saved to vocab.toml; applied to every segment and handed to the LLM as preferred spellings.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// What the recognizer writes ("" = a term only the LLM uses).
    pub heard: String,
    /// What the user wants instead.
    pub write: String,
    /// How many times the user has made this edit.
    #[serde(default)]
    pub seen: u32,
    /// Learned but not trusted yet: not applied until the LLM (or repetition) says it's a real mishearing.
    #[serde(default)]
    pub pending: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Vocab {
    #[serde(default)]
    pub entries: Vec<Entry>,
}

const MAX_TOKENS: usize = 6; // longer edits are rewrites, not word fixes

fn path() -> std::path::PathBuf {
    crate::config::app_dir().join("vocab.toml")
}

impl Vocab {
    pub fn load() -> Self {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|s| toml::from_str(&s).map_err(|e| log::warn!("vocab.toml: {e}")).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Err(e) = std::fs::write(path(), toml::to_string_pretty(self).unwrap()) {
            log::error!("save vocab: {e}");
        }
    }

    /// Replace every known mishearing, longest first.
    pub fn apply(&self, text: &str) -> String {
        let mut es: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| !e.pending && !e.heard.is_empty() && !e.write.is_empty())
            .collect();
        es.sort_by_key(|e| std::cmp::Reverse(e.heard.len()));
        es.iter().fold(text.to_string(), |t, e| replace(&t, &e.heard, &e.write))
    }

    /// Extra system-prompt lines for the LLM ("" when empty).
    pub fn prompt(&self) -> String {
        let lines: Vec<String> = self
            .entries
            .iter()
            .filter(|e| !e.pending && !e.write.is_empty())
            .map(|e| {
                if e.heard.is_empty() {
                    format!("- {}", e.write)
                } else {
                    format!("- {} (not {})", e.write, e.heard)
                }
            })
            .collect();
        if lines.is_empty() {
            return String::new();
        }
        format!(
            "\nThe user's names and terms. Use these exact spellings, also when the speech sounds similar to one of them:\n{}",
            lines.join("\n")
        )
    }

    /// Count the word-level fixes between what was shown and what the user pasted. New ones start `pending`;
    /// returns the pending ones just seen, for the caller to decide on (then `confirm`).
    pub fn learn(&mut self, shown: &str, pasted: &str) -> Vec<Entry> {
        let mut out = Vec::new();
        for (heard, write) in word_fixes(shown, pasted) {
            log::info!("vocab: saw {heard:?} -> {write:?}");
            self.entries.retain(|e| !(e.heard == write && e.write == heard)); // undoing a swap drops it
            let i = self
                .entries
                .iter()
                .position(|e| e.heard == heard && e.write == write)
                .unwrap_or_else(|| {
                    self.entries.push(Entry {
                        heard,
                        write,
                        seen: 0,
                        pending: true,
                    });
                    self.entries.len() - 1
                });
            self.entries[i].seen += 1;
            if self.entries[i].pending {
                out.push(self.entries[i].clone());
            }
        }
        out
    }

    /// Start applying a pending swap; it replaces any other spelling for the same heard text.
    pub fn confirm(&mut self, heard: &str, write: &str) {
        self.entries.retain(|e| e.heard != heard || e.write == write);
        for e in &mut self.entries {
            if e.heard == heard {
                e.pending = false;
            }
        }
    }
}

fn is_word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric())
}

/// Case-insensitive replace; English words only match whole words ("Lee" doesn't hit "Leeds").
fn replace(text: &str, from: &str, to: &str) -> String {
    let (lt, lf) = (text.to_ascii_lowercase(), from.to_ascii_lowercase()); // ASCII lowering keeps byte offsets
    let (wa, wb) = (is_word(from.chars().next()), is_word(from.chars().last()));
    let (mut out, mut i) = (String::new(), 0);
    while let Some(k) = lt[i..].find(&lf) {
        let (at, end) = (i + k, i + k + lf.len());
        let whole = !(wa && is_word(text[..at].chars().last())) && !(wb && is_word(text[end..].chars().next()));
        out.push_str(&text[i..at]);
        out.push_str(if whole { to } else { &text[at..end] });
        i = end;
    }
    out.push_str(&text[i..]);
    out
}

/// Tokens as byte ranges: an ASCII word, or any other single non-space character (so each CJK char is a token).
fn tokens(s: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c.is_whitespace() {
            continue;
        }
        let mut end = i + c.len_utf8();
        if c.is_ascii_alphanumeric() {
            while let Some(&(j, d)) = it.peek() {
                if d.is_ascii_alphanumeric() || d == '\'' {
                    end = j + d.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
        }
        out.push((i, end));
    }
    out
}

/// Replaced spans between two texts (token LCS diff), filtered to short word-like swaps.
fn word_fixes(a: &str, b: &str) -> Vec<(String, String)> {
    let (ta, tb) = (tokens(a), tokens(b));
    let (sa, sb): (Vec<&str>, Vec<&str>) = (
        ta.iter().map(|r| &a[r.0..r.1]).collect(),
        tb.iter().map(|r| &b[r.0..r.1]).collect(),
    );
    let (n, m) = (sa.len(), sb.len());
    if n * m > 4_000_000 {
        return Vec::new();
    } // shortcut: quadratic diff, skip huge pastes
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if sa[i] == sb[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let cjk = |s: &str| s.chars().next().is_some_and(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && sa[i] == sb[j] {
            i += 1;
            j += 1;
            continue;
        }
        let (i0, j0) = (i, j);
        while (i < n || j < m) && !(i < n && j < m && sa[i] == sb[j]) {
            if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
                j += 1;
            } else {
                i += 1;
            }
        }
        let (mut i0, mut j0, mut i1, mut j1) = (i0, j0, i, j);
        if i1 == i0 || j1 == j0 || i1 - i0 > MAX_TOKENS || j1 - j0 > MAX_TOKENS {
            continue;
        }
        // a single changed CJK char ("力"→"立") is too generic on its own: take the neighbouring characters with it
        if i1 - i0 == 1 && cjk(sa[i0]) {
            if i0 > 0 && j0 > 0 && cjk(sa[i0 - 1]) {
                i0 -= 1;
                j0 -= 1;
            }
            if i1 < n && j1 < m && cjk(sa[i1]) {
                i1 += 1;
                j1 += 1;
            }
        }
        let (heard, write) = (&a[ta[i0].0..ta[i1 - 1].1], &b[tb[j0].0..tb[j1 - 1].1]);
        if heard != write && heard.chars().any(char::is_alphanumeric) && write.chars().any(char::is_alphanumeric) {
            out.push((heard.to_string(), write.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn learns_word_swaps() {
        assert_eq!(
            word_fixes("I met Mark League today.", "I met Mark Lee today."),
            vec![("League".into(), "Lee".into())]
        );
        assert_eq!(
            word_fixes("我跟李力凱開會", "我跟李立凱開會"),
            vec![("李力凱".into(), "李立凱".into())]
        );
        assert_eq!(word_fixes("ok，好", "ok。好"), vec![]); // punctuation only
        assert_eq!(word_fixes("a b c", "a b c d e"), vec![]); // pure insertion (dictated text)
        assert!(
            word_fixes(
                "one two three four five six seven",
                "totally different words that replace everything here"
            )
            .is_empty()
        );
    }
    #[test]
    fn applies_whole_words_case_insensitive() {
        let v = Vocab {
            entries: vec![
                Entry {
                    heard: "league".into(),
                    write: "Lee".into(),
                    ..Default::default()
                },
                Entry {
                    heard: "李力凱".into(),
                    write: "李立凱".into(),
                    ..Default::default()
                },
            ],
        };
        assert_eq!(
            v.apply("Mark League and the leagues, 李力凱說"),
            "Mark Lee and the leagues, 李立凱說"
        );
        let mut v = v;
        v.learn("Lee", "league"); // reverting removes the old pair instead of looping
        assert_eq!(
            v.entries
                .iter()
                .filter(|e| e.heard == "league" || e.heard == "Lee")
                .count(),
            1
        );
        assert!(v.prompt().contains("李立凱 (not 李力凱)"));
    }
    #[test]
    fn learned_swaps_wait_for_confirmation() {
        let mut v = Vocab::default();
        assert_eq!(v.learn("go to it", "go do it")[0].seen, 1);
        assert_eq!(
            v.learn("to me", "do me")[0].seen,
            2,
            "same edit is counted, not duplicated"
        );
        assert_eq!(v.apply("to"), "to", "pending swaps are not applied");
        assert_eq!(v.prompt(), "", "nor given to the LLM");
        v.confirm("to", "do");
        assert_eq!(v.apply("to"), "do");
        assert!(
            v.learn("to me", "do me").is_empty(),
            "already confirmed: nothing to decide"
        );
    }
}
