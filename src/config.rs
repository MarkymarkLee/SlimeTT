use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// "RightOption", "RightCommand", or a chord like "Ctrl+Alt+Space".
    pub hotkey: String,
    /// Optional second hotkey, same format; "" = none.
    pub hotkey2: String,
    /// Key into asr::CATALOG used for final segment text.
    pub asr_model: String,
    /// Optional streaming model (asr::CATALOG, online kind) for live gray text. "" = rolling re-transcription with asr_model.
    pub asr_streaming_model: String,
    pub llm_enabled: bool,
    /// "apple" = on-device Apple Foundation Models (the app runs `fm serve`); "openai" = any OpenAI-compatible endpoint at llm_url/llm_model.
    pub llm_backend: String,
    pub llm_url: String,
    pub llm_model: String,
    pub polish: bool,
    /// "s2twp" | "s2hk" | "off"
    pub opencc: String,
    /// "bottom" | "top"
    pub popup_position: String,
    /// "" = system default input device
    pub input_device: String,
    pub sounds: bool,
    /// Welcome/permissions window was completed or dismissed once.
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "RightOption".into(),
            hotkey2: String::new(),
            asr_model: "x-asr-offline-int8".into(),
            asr_streaming_model: String::new(),
            llm_enabled: true,
            llm_backend: "apple".into(),
            llm_url: "http://127.0.0.1:11434".into(),
            llm_model: "Qwen3.6-35B-A3B-MLX-4bit".into(),
            polish: false,
            opencc: "s2twp".into(),
            popup_position: "bottom".into(),
            input_device: String::new(),
            sounds: true,
            onboarded: false,
        }
    }
}

pub fn app_dir() -> PathBuf {
    let d = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("stt");
    std::fs::create_dir_all(&d).ok();
    d
}

pub fn models_dir() -> PathBuf {
    let d = app_dir().join("models");
    std::fs::create_dir_all(&d).ok();
    d
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(app_dir().join("settings.toml"))
            .ok()
            .and_then(|s| toml::from_str(&s).map_err(|e| log::warn!("settings.toml: {e}")).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Err(e) = std::fs::write(app_dir().join("settings.toml"), toml::to_string_pretty(self).unwrap()) {
            log::error!("save settings: {e}");
        }
    }
}
