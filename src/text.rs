//! Final text step (OpenCC) and the two instant voice commands that bypass the LLM.
use opencc_fmmseg::OpenCC;
use std::sync::OnceLock;

static OPENCC: OnceLock<OpenCC> = OnceLock::new();

/// Convert to Traditional Chinese per the `opencc` setting ("s2twp", "s2hk", "off").
pub fn finalize(text: &str, mode: &str) -> String {
    // the X-ASR punct model emits "， " — drop the space after full-width punctuation
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c == ' ' && out.chars().last().is_some_and(|p| "，。？！：；、".contains(p)) {
            continue;
        }
        out.push(c);
    }
    if mode == "off" || !out.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
        return out;
    }
    OPENCC.get_or_init(OpenCC::new).convert(&out, mode, false)
}

#[derive(Debug, PartialEq)]
pub enum Instant {
    Undo,
    Clear,
}

/// A whole finalized segment that is exactly "undo"/"clear" (zh or en) is a command, not text.
pub fn instant_command(segment: &str) -> Option<Instant> {
    let s: String = segment
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    match s.as_str() {
        "undo" | "復原" | "复原" | "撤銷" | "撤销" => Some(Instant::Undo),
        "clear" | "clearall" | "清除" | "全部清除" | "全部刪掉" | "全部删掉" => Some(Instant::Clear),
        _ => None,
    }
}

/// Join finalized segments: no space between CJK neighbours, a single space otherwise; "\n" segments are line breaks.
pub fn join(segments: &[String]) -> String {
    let cjk = |c: char| ('\u{4e00}'..='\u{9fff}').contains(&c) || "，。？！：；、".contains(c);
    let mut out = String::new();
    for s in segments {
        if s == "\n" {
            out.push('\n');
            continue;
        }
        if let (Some(a), Some(b)) = (out.chars().last(), s.chars().next())
            && a != '\n'
            && !(cjk(a) && cjk(b))
        {
            out.push(' ');
        }
        out.push_str(s);
    }
    out
}

/// Insert a dictated segment at char index `caret` of `buf` (same spacing rules as `join`); returns the new caret.
pub fn insert_at(buf: &mut String, caret: usize, seg: &str) -> usize {
    let at = buf.char_indices().nth(caret).map_or(buf.len(), |(i, _)| i);
    let (left, right) = buf.split_at(at);
    let head = join(&[left.trim_end_matches(' ').to_string(), seg.to_string()]); // `join` re-adds the space where one belongs
    let new_caret = head.chars().count();
    *buf = join(&[head, right.trim_start_matches(' ').to_string()]);
    new_caret
}

#[cfg(test)]
#[path = "../tests/text.rs"]
mod tests;
