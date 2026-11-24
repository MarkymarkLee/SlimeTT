//! Dictation engine thread: audio -> VAD -> provisional/final text events.
use crate::{asr, audio, config::Settings};
use anyhow::Result;
use std::{
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

pub enum Cmd {
    Start,
    Stop,
    Reload(Box<Settings>),
    Quit,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// Mic RMS level 0..1 while recording, for the blob.
    Level(f32),
    /// Live (gray) text of the segment being spoken.
    Provisional(String),
    /// A finished segment (raw ASR output).
    Final(String),
    /// Human-readable status (model loading, download progress) or "" when ready.
    Status(String),
    Error(String),
    /// Reply to `Cmd::Stop`: every `Final` for the audio captured so far has been sent.
    Stopped,
}

const ROLLING_INTERVAL: Duration = Duration::from_millis(300); // offline pass costs ~15-50 ms per 6 s of speech
/// Audio kept from before the VAD fires (it needs min_speech_duration of speech first), replayed into the
/// live model so the first word isn't lost and the preview starts right away.
const PREROLL: usize = 16000 * 6 / 10;
const THREADS: i32 = 8;

pub fn spawn(settings: Settings, events: Sender<Event>) -> Sender<Cmd> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("engine".into())
        .spawn(move || run(settings, rx, events))
        .unwrap();
    tx
}

struct Models {
    offline: asr::Offline,
    online: Option<asr::Online>,
    vad: asr::Vad,
}

fn load(s: &Settings, events: &Sender<Event>) -> Result<Models> {
    let status = |m: String| {
        events.send(Event::Status(m)).ok();
    };
    let progress = |what: &str| {
        let (ev, what) = (events.clone(), what.to_string());
        move |done: u64, total: u64| {
            ev.send(Event::Status(format!(
                "Downloading the {what}: {} / {} MB. Dictation starts when this is done.",
                done >> 20,
                total >> 20
            )))
            .ok();
        }
    };
    let spec = asr::spec(&s.asr_model).ok_or_else(|| anyhow::anyhow!("unknown model {}", s.asr_model))?;
    let dir = asr::ensure(spec, &progress("speech model"))?;
    let vad_path = asr::ensure_vad(&progress("voice detector"))?;
    status("Getting the speech model ready…".into());
    let offline = asr::Offline::load(spec, &dir, THREADS)?;
    let online = match asr::spec(&s.asr_streaming_model) {
        Some(sp) => {
            let d = asr::ensure(sp, &progress("live-preview model"))?;
            Some(asr::Online::load(sp, &d, THREADS)?)
        }
        None => None,
    };
    let vad = asr::Vad::load(&vad_path)?;
    status(String::new());
    Ok(Models { offline, online, vad })
}

fn run(mut settings: Settings, rx: Receiver<Cmd>, events: Sender<Event>) {
    let mut models = load(&settings, &events)
        .map_err(|e| {
            events.send(Event::Error(format!(
                "Couldn't load the speech model, so nothing can be transcribed: {e:#}"
            )))
        })
        .ok();
    let (atx, arx) = mpsc::channel::<Vec<f32>>();
    let mut stream: Option<cpal::Stream> = None;
    let mut pending: Vec<f32> = Vec::new(); // samples not yet fed to the VAD (multiple of VAD_WINDOW)
    let mut current: Vec<f32> = Vec::new(); // audio of the segment in progress (for rolling re-transcription)
    let mut preroll: Vec<f32> = Vec::new(); // last PREROLL samples heard while not speaking
    let mut last_roll = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Cmd::Quit) => return,
            Ok(Cmd::Reload(s)) => {
                let need = s.asr_model != settings.asr_model || s.asr_streaming_model != settings.asr_streaming_model;
                settings = *s;
                if need {
                    models = load(&settings, &events)
                        .map_err(|e| {
                            events.send(Event::Error(format!(
                                "Couldn't load the speech model, so nothing can be transcribed: {e:#}"
                            )))
                        })
                        .ok();
                }
            }
            Ok(Cmd::Start) => {
                if let Some(m) = &models {
                    m.vad.reset();
                    if let Some(o) = &m.online {
                        o.reset();
                    }
                }
                current.clear();
                pending.clear();
                preroll.clear();
                while arx.try_recv().is_ok() {}
                match audio::start(&settings.input_device, atx.clone()) {
                    Ok(s) => stream = Some(s),
                    Err(e) => {
                        events
                            .send(Event::Error(format!("Can't use the microphone: {e:#}")))
                            .ok();
                    }
                }
            }
            Ok(Cmd::Stop) => {
                stream = None;
                if let Some(m) = &models {
                    while let Ok(chunk) = arx.try_recv() {
                        feed(m, &chunk, &mut pending, &mut current, &mut preroll, &events);
                    }
                    m.vad.flush();
                    finalize_segments(m, &mut current, &events);
                    events.send(Event::Level(0.0)).ok();
                }
                events.send(Event::Stopped).ok();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if stream.is_none() {
            continue;
        }
        let Some(m) = &models else { continue };
        while let Ok(chunk) = arx.try_recv() {
            let rms = (chunk.iter().map(|x| x * x).sum::<f32>() / chunk.len().max(1) as f32).sqrt();
            events.send(Event::Level((rms * 6.0).min(1.0))).ok();
            feed(m, &chunk, &mut pending, &mut current, &mut preroll, &events);
        }
        // rolling re-transcription only when there is no streaming model
        if m.online.is_none() && !current.is_empty() && last_roll.elapsed() > ROLLING_INTERVAL && current.len() > 4800 {
            events.send(Event::Provisional(m.offline.transcribe(&current))).ok();
            last_roll = Instant::now();
        }
    }
}

fn feed(
    m: &Models,
    chunk: &[f32],
    pending: &mut Vec<f32>,
    current: &mut Vec<f32>,
    preroll: &mut Vec<f32>,
    events: &Sender<Event>,
) {
    pending.extend_from_slice(chunk);
    let n = pending.len() / asr::VAD_WINDOW * asr::VAD_WINDOW;
    for w in pending[..n].chunks(asr::VAD_WINDOW) {
        m.vad.accept(w);
        if m.vad.speaking() {
            if current.is_empty() {
                // speech just started: replay what came before the VAD noticed
                current.append(preroll);
                if let Some(o) = &m.online {
                    o.push(current);
                }
            }
            current.extend_from_slice(w);
            if let Some(o) = &m.online {
                events.send(Event::Provisional(o.push(w))).ok();
            }
        } else {
            preroll.extend_from_slice(w);
            let extra = preroll.len().saturating_sub(PREROLL);
            preroll.drain(..extra);
        }
    }
    pending.drain(..n);
    finalize_segments(m, current, events);
}

fn finalize_segments(m: &Models, current: &mut Vec<f32>, events: &Sender<Event>) {
    for seg in m.vad.segments() {
        // the model returns nothing for a bare ~0.2 s word like "喔"; 0.3 s of silence around short segments fixes it
        let seg = if seg.len() < 16000 {
            [vec![0.0; 4800], seg, vec![0.0; 4800]].concat()
        } else {
            seg
        };
        let text = m.offline.transcribe(&seg);
        if let Some(o) = &m.online {
            o.reset();
        }
        current.clear();
        events.send(Event::Provisional(String::new())).ok();
        if !text.is_empty() {
            events.send(Event::Final(text)).ok();
        }
    }
}
