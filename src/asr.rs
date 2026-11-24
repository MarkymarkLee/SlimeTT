//! Safe wrappers over the sherpa-onnx C API plus the model catalog.
use crate::sherpa_sys::*;
use anyhow::{Context, Result, anyhow};
use std::{
    ffi::{CStr, CString},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    SenseVoice,
    Qwen3Asr,
    FireRedAsr,
    FireRedAsrCtc,
    FunAsrNano,
    Whisper,
    OfflineTransducer,
    OnlineTransducer,
}

pub struct ModelSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub archive: &'static str,
    pub kind: Kind,
}

pub const CATALOG: &[ModelSpec] = &[
    ModelSpec {
        key: "sense-voice-int8",
        label: "SenseVoice small int8 (fast)",
        archive: "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09",
        kind: Kind::SenseVoice,
    },
    ModelSpec {
        key: "sense-voice",
        label: "SenseVoice small fp32",
        archive: "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2025-09-09",
        kind: Kind::SenseVoice,
    },
    ModelSpec {
        key: "qwen3-asr-0.6b-int8",
        label: "Qwen3-ASR 0.6B int8",
        archive: "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25",
        kind: Kind::Qwen3Asr,
    },
    ModelSpec {
        key: "fire-red-asr2-int8",
        label: "FireRedASR2 AED int8",
        archive: "sherpa-onnx-fire-red-asr2-zh_en-int8-2026-02-26",
        kind: Kind::FireRedAsr,
    },
    ModelSpec {
        key: "fire-red-asr2-ctc-int8",
        label: "FireRedASR2 CTC int8",
        archive: "sherpa-onnx-fire-red-asr2-ctc-zh_en-int8-2026-02-25",
        kind: Kind::FireRedAsrCtc,
    },
    ModelSpec {
        key: "funasr-nano-int8",
        label: "FunASR Nano int8",
        archive: "sherpa-onnx-funasr-nano-int8-2025-12-30",
        kind: Kind::FunAsrNano,
    },
    ModelSpec {
        key: "whisper-turbo",
        label: "Whisper large-v3-turbo (slow on CPU)",
        archive: "sherpa-onnx-whisper-turbo",
        kind: Kind::Whisper,
    },
    ModelSpec {
        key: "x-asr-offline-int8",
        label: "X-ASR zh-en offline int8",
        archive: "sherpa-onnx-x-asr-zipformer-transducer-zh-en-punct-int8-2026-06-03",
        kind: Kind::OfflineTransducer,
    },
    ModelSpec {
        key: "x-asr-480ms-streaming-int8",
        label: "X-ASR zh-en streaming 480ms int8",
        archive: "sherpa-onnx-x-asr-480ms-streaming-zipformer-transducer-zh-en-punct-int8-2026-06-05",
        kind: Kind::OnlineTransducer,
    },
    ModelSpec {
        key: "zipformer-multilingual-streaming",
        label: "Zipformer multilingual streaming",
        archive: "sherpa-onnx-streaming-zipformer-ar_en_id_ja_ru_th_vi_zh-2025-02-10",
        kind: Kind::OnlineTransducer,
    },
];

pub fn spec(key: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|m| m.key == key)
}

/// First file in `dir` whose name contains `pat` and ends with `ext`, preferring int8 variants.
fn pick(dir: &Path, pat: &str, ext: &str) -> Result<String> {
    let mut hits: Vec<PathBuf> = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains(pat) && n.ends_with(ext))
        })
        .collect();
    hits.sort_by_key(|p| !p.to_string_lossy().contains("int8"));
    hits.first()
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow!("no {pat}*{ext} in {}", dir.display()))
}

/// Owns the CStrings a config points at; `c()` hands out stable pointers.
#[derive(Default)]
struct Strs(Vec<CString>);
impl Strs {
    fn c(&mut self, s: &str) -> *const std::os::raw::c_char {
        self.0.push(CString::new(s).unwrap());
        self.0.last().unwrap().as_ptr()
    }
}

pub struct Offline {
    ptr: *const SherpaOnnxOfflineRecognizer,
    _s: Strs,
}
unsafe impl Send for Offline {}
impl Drop for Offline {
    fn drop(&mut self) {
        unsafe { SherpaOnnxDestroyOfflineRecognizer(self.ptr) }
    }
}

impl Offline {
    pub fn load(spec: &ModelSpec, dir: &Path, threads: i32) -> Result<Self> {
        let mut s = Strs::default();
        let mut cfg = SherpaOnnxOfflineRecognizerConfig {
            feat_config: SherpaOnnxFeatureConfig {
                sample_rate: 16000,
                feature_dim: 80,
            },
            ..Default::default()
        };
        cfg.decoding_method = s.c("greedy_search");
        let m = &mut cfg.model_config;
        m.num_threads = threads;
        m.provider = s.c("cpu");
        m.modeling_unit = s.c("cjkchar");
        match spec.kind {
            Kind::SenseVoice => {
                m.sense_voice = SherpaOnnxOfflineSenseVoiceModelConfig {
                    model: s.c(&pick(dir, "model", ".onnx")?),
                    language: s.c("auto"),
                    use_itn: 1,
                };
                m.tokens = s.c(&pick(dir, "tokens", ".txt")?);
            }
            Kind::Qwen3Asr => {
                m.qwen3_asr = SherpaOnnxOfflineQwen3ASRModelConfig {
                    conv_frontend: s.c(&pick(dir, "conv_frontend", ".onnx")?),
                    encoder: s.c(&pick(dir, "encoder", ".onnx")?),
                    decoder: s.c(&pick(dir, "decoder", ".onnx")?),
                    tokenizer: s.c(&dir.join("tokenizer").to_string_lossy()),
                    max_total_len: 512,
                    max_new_tokens: 128,
                    temperature: 1e-6,
                    top_p: 0.8,
                    seed: 42,
                    hotwords: s.c(""),
                };
                cfg.feat_config.feature_dim = 128;
            }
            Kind::FireRedAsr => {
                m.fire_red_asr = SherpaOnnxOfflineFireRedAsrModelConfig {
                    encoder: s.c(&pick(dir, "encoder", ".onnx")?),
                    decoder: s.c(&pick(dir, "decoder", ".onnx")?),
                };
                m.tokens = s.c(&pick(dir, "tokens", ".txt")?);
            }
            Kind::FireRedAsrCtc => {
                m.fire_red_asr_ctc = SherpaOnnxOfflineFireRedAsrCtcModelConfig {
                    model: s.c(&pick(dir, "model", ".onnx")?),
                };
                m.tokens = s.c(&pick(dir, "tokens", ".txt")?);
            }
            Kind::FunAsrNano => {
                m.funasr_nano = SherpaOnnxOfflineFunASRNanoModelConfig {
                    encoder_adaptor: s.c(&pick(dir, "encoder_adaptor", ".onnx")?),
                    llm: s.c(&pick(dir, "llm", ".onnx")?),
                    embedding: s.c(&pick(dir, "embedding", ".onnx")?),
                    tokenizer: s.c(&dir.join("Qwen3-0.6B").to_string_lossy()),
                    system_prompt: s.c("You are a helpful assistant."),
                    user_prompt: s.c("语音转写:"),
                    max_new_tokens: 512,
                    temperature: 1e-6,
                    top_p: 0.8,
                    seed: 42,
                    language: s.c(""),
                    itn: 1,
                    hotwords: s.c(""),
                };
            }
            Kind::Whisper => {
                m.whisper = SherpaOnnxOfflineWhisperModelConfig {
                    encoder: s.c(&pick(dir, "encoder", ".onnx")?),
                    decoder: s.c(&pick(dir, "decoder", ".onnx")?),
                    language: s.c(""),
                    task: s.c("transcribe"),
                    tail_paddings: -1,
                    enable_token_timestamps: 0,
                    enable_segment_timestamps: 0,
                };
                m.tokens = s.c(&pick(dir, "tokens", ".txt")?);
            }
            Kind::OfflineTransducer => {
                m.transducer = SherpaOnnxOfflineTransducerModelConfig {
                    encoder: s.c(&pick(dir, "encoder", ".onnx")?),
                    decoder: s.c(&pick(dir, "decoder", ".onnx")?),
                    joiner: s.c(&pick(dir, "joiner", ".onnx")?),
                };
                m.tokens = s.c(&pick(dir, "tokens", ".txt")?);
            }
            Kind::OnlineTransducer => return Err(anyhow!("{} is a streaming model", spec.key)),
        }
        let ptr = unsafe { SherpaOnnxCreateOfflineRecognizer(&cfg) };
        if ptr.is_null() {
            return Err(anyhow!("sherpa-onnx refused config for {}", spec.key));
        }
        Ok(Self { ptr, _s: s })
    }

    pub fn transcribe(&self, samples: &[f32]) -> String {
        unsafe {
            let st = SherpaOnnxCreateOfflineStream(self.ptr);
            SherpaOnnxAcceptWaveformOffline(st, 16000, samples.as_ptr(), samples.len() as i32);
            SherpaOnnxDecodeOfflineStream(self.ptr, st);
            let r = SherpaOnnxGetOfflineStreamResult(st);
            let text = CStr::from_ptr((*r).text).to_string_lossy().trim().to_string();
            SherpaOnnxDestroyOfflineRecognizerResult(r);
            SherpaOnnxDestroyOfflineStream(st);
            text
        }
    }
}

pub struct Online {
    rec: *const SherpaOnnxOnlineRecognizer,
    stream: *const SherpaOnnxOnlineStream,
    _s: Strs,
}
unsafe impl Send for Online {}
impl Drop for Online {
    fn drop(&mut self) {
        unsafe {
            SherpaOnnxDestroyOnlineStream(self.stream);
            SherpaOnnxDestroyOnlineRecognizer(self.rec)
        }
    }
}

impl Online {
    pub fn load(spec: &ModelSpec, dir: &Path, threads: i32) -> Result<Self> {
        if spec.kind != Kind::OnlineTransducer {
            return Err(anyhow!("{} is not a streaming model", spec.key));
        }
        let mut s = Strs::default();
        let mut cfg = SherpaOnnxOnlineRecognizerConfig {
            feat_config: SherpaOnnxFeatureConfig {
                sample_rate: 16000,
                feature_dim: 80,
            },
            ..Default::default()
        };
        cfg.decoding_method = s.c("greedy_search");
        cfg.max_active_paths = 4;
        cfg.model_config.transducer = SherpaOnnxOnlineTransducerModelConfig {
            encoder: s.c(&pick(dir, "encoder", ".onnx")?),
            decoder: s.c(&pick(dir, "decoder", ".onnx")?),
            joiner: s.c(&pick(dir, "joiner", ".onnx")?),
        };
        cfg.model_config.tokens = s.c(&pick(dir, "tokens", ".txt")?);
        cfg.model_config.num_threads = threads;
        cfg.model_config.provider = s.c("cpu");
        cfg.model_config.modeling_unit = s.c("cjkchar");
        let rec = unsafe { SherpaOnnxCreateOnlineRecognizer(&cfg) };
        if rec.is_null() {
            return Err(anyhow!("sherpa-onnx refused config for {}", spec.key));
        }
        let stream = unsafe { SherpaOnnxCreateOnlineStream(rec) };
        Ok(Self { rec, stream, _s: s })
    }

    /// Feed audio, return the current partial text for the segment.
    pub fn push(&self, samples: &[f32]) -> String {
        unsafe {
            SherpaOnnxOnlineStreamAcceptWaveform(self.stream, 16000, samples.as_ptr(), samples.len() as i32);
            while SherpaOnnxIsOnlineStreamReady(self.rec, self.stream) != 0 {
                SherpaOnnxDecodeOnlineStream(self.rec, self.stream);
            }
            let r = SherpaOnnxGetOnlineStreamResult(self.rec, self.stream);
            let text = CStr::from_ptr((*r).text).to_string_lossy().trim().to_string();
            SherpaOnnxDestroyOnlineRecognizerResult(r);
            text
        }
    }
    pub fn reset(&self) {
        unsafe { SherpaOnnxOnlineStreamReset(self.rec, self.stream) }
    }
}

pub struct Vad {
    ptr: *const SherpaOnnxVoiceActivityDetector,
    _s: Strs,
}
unsafe impl Send for Vad {}
impl Drop for Vad {
    fn drop(&mut self) {
        unsafe { SherpaOnnxDestroyVoiceActivityDetector(self.ptr) }
    }
}

pub const VAD_WINDOW: usize = 512;

impl Vad {
    /// Silero VAD: 600 ms silence ends a segment; 15 s hard cap force-finalizes.
    pub fn load(model: &Path) -> Result<Self> {
        let mut s = Strs::default();
        let mut cfg = SherpaOnnxVadModelConfig {
            silero_vad: SherpaOnnxSileroVadModelConfig {
                model: s.c(&model.to_string_lossy()),
                threshold: 0.5,
                min_silence_duration: 0.6,
                min_speech_duration: 0.1, // low so a lone "喔" counts
                window_size: VAD_WINDOW as i32,
                max_speech_duration: 15.0,
            },
            ..Default::default()
        };
        cfg.sample_rate = 16000;
        cfg.num_threads = 1;
        cfg.provider = s.c("cpu");
        let ptr = unsafe { SherpaOnnxCreateVoiceActivityDetector(&cfg, 60.0) };
        if ptr.is_null() {
            return Err(anyhow!("vad load failed"));
        }
        Ok(Self { ptr, _s: s })
    }
    pub fn accept(&self, window: &[f32]) {
        unsafe { SherpaOnnxVoiceActivityDetectorAcceptWaveform(self.ptr, window.as_ptr(), window.len() as i32) }
    }
    pub fn speaking(&self) -> bool {
        unsafe { SherpaOnnxVoiceActivityDetectorDetected(self.ptr) != 0 }
    }
    pub fn flush(&self) {
        unsafe { SherpaOnnxVoiceActivityDetectorFlush(self.ptr) }
    }
    pub fn reset(&self) {
        unsafe { SherpaOnnxVoiceActivityDetectorReset(self.ptr) }
    }
    /// Pop all completed speech segments.
    pub fn segments(&self) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        unsafe {
            while SherpaOnnxVoiceActivityDetectorEmpty(self.ptr) == 0 {
                let seg = SherpaOnnxVoiceActivityDetectorFront(self.ptr);
                out.push(std::slice::from_raw_parts((*seg).samples, (*seg).n as usize).to_vec());
                SherpaOnnxDestroySpeechSegment(seg);
                SherpaOnnxVoiceActivityDetectorPop(self.ptr);
            }
        }
        out
    }
}

/// Resolve the models directory for a spec, downloading if needed.
pub fn ensure(spec: &ModelSpec, progress: &dyn Fn(u64, u64)) -> Result<PathBuf> {
    crate::download::ensure_archive(&crate::config::models_dir(), spec.archive, progress)
        .with_context(|| spec.key.to_string())
}
pub fn ensure_vad(progress: &dyn Fn(u64, u64)) -> Result<PathBuf> {
    crate::download::ensure_file(&crate::config::models_dir(), "silero_vad.onnx", progress)
}
