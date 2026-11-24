//! OpenAI-compatible chat client for the cleanup pass. Apple Foundation Models is reached the same way,
//! through Apple's `fm serve`, which the app starts itself.
use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

pub const STRICT_PROMPT: &str = include_str!("../assets/prompts/cleanup.txt");
pub const POLISH_EXTRA: &str = include_str!("../assets/prompts/polish.txt");

/// Where the app-managed `fm serve` listens, and the model name it serves.
pub const FM_URL: &str = "http://127.0.0.1:11976";
pub const FM_MODEL: &str = "system";

/// Start Apple's on-device model server. None if `fm` is missing (macOS < 26).
pub fn spawn_fm() -> Option<std::process::Child> {
    use std::process::{Command, Stdio};
    Command::new("/usr/bin/fm")
        .args(["serve", "--host", "127.0.0.1", "--port", "11976"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| log::warn!("fm serve: {e}"))
        .ok()
}

pub fn healthy(url: &str) -> bool {
    ureq::get(&format!("{url}/health"))
        .config()
        .timeout_global(Some(Duration::from_millis(800)))
        .build()
        .call()
        .is_ok()
}

pub fn models(url: &str) -> Vec<String> {
    #[derive(Deserialize)]
    struct M {
        id: String,
    }
    #[derive(Deserialize)]
    struct R {
        data: Vec<M>,
    }
    ureq::get(&format!("{url}/v1/models"))
        .config()
        .timeout_global(Some(Duration::from_secs(3)))
        .build()
        .call()
        .ok()
        .and_then(|mut r| r.body_mut().read_json::<R>().ok())
        .map(|r| {
            r.data
                .into_iter()
                .map(|m| m.id)
                .filter(|id| !id.to_lowercase().contains("asr") && !id.contains("parakeet"))
                .collect()
        })
        .unwrap_or_default()
}

pub fn cleanup(url: &str, model: &str, polish: bool, vocab: &str, raw: &str) -> Result<String> {
    let system = if polish {
        format!("{STRICT_PROMPT}\n{POLISH_EXTRA}{vocab}")
    } else {
        format!("{STRICT_PROMPT}{vocab}")
    };
    chat(url, model, &system, raw)
}

const KEEP_FIX_PROMPT: &str = "You maintain the auto-replace list of a dictation app. The user edited the speech recognizer's output. \
Decide whether that edit should be applied automatically to everything they dictate from now on.
ALWAYS: the recognizer misheard a name, a technical term or a sound-alike word, and the heard text is not something the user would normally want written (\"League\" -> \"Lee\", \"Any Gamer\" -> \"AniGamer\").
ONCE: the heard text is an ordinary word or phrase that is often correct as it is (\"to\" -> \"do\", \"if we were\" -> \"everywhere\"), or the user changed the meaning or reworded.
The same edit seen several times is evidence for ALWAYS, but never enough for an everyday word.
Answer with one word: ALWAYS or ONCE.";

/// Ask the model whether a learned swap is a real mishearing worth applying every time (true) or a one-off edit.
pub fn keep_fix(
    url: &str,
    model: &str,
    heard: &str,
    write: &str,
    seen: u32,
    before: &str,
    after: &str,
) -> Result<bool> {
    let user = format!(
        "heard: {heard}\nuser changed it to: {write}\ntimes seen: {seen}\ntext before the edit: {before}\ntext after the edit: {after}"
    );
    Ok(chat(url, model, KEEP_FIX_PROMPT, &user)?
        .to_uppercase()
        .contains("ALWAYS"))
}

fn chat(url: &str, model: &str, system: &str, user: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct Msg {
        content: String,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: Msg,
    }
    #[derive(Deserialize)]
    struct R {
        choices: Vec<Choice>,
    }
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "temperature": 0, "max_tokens": 1024, "stream": false, // fm serve streams unless told not to
        "chat_template_kwargs": {"enable_thinking": false}
    });
    let r: R = ureq::post(&format!("{url}/v1/chat/completions"))
        .config()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .send_json(body)
        .context("llm request")?
        .body_mut()
        .read_json()
        .context("llm response")?;
    let out = r.choices.into_iter().next().context("no choices")?.message.content;
    // guard: strip a leaked thinking block if the template ignored enable_thinking
    let out = match out.rfind("</think>") {
        Some(i) => &out[i + 8..],
        None => &out[..],
    };
    Ok(out.trim().to_string())
}
