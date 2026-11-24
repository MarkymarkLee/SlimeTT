#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod asr;
mod audio;
mod blob;
mod config;
mod download;
mod engine;
mod llm;
mod macos;
mod sherpa_sys;
mod text;
mod theme;
mod vocab;

use config::Settings;
use eframe::egui::{self, Color32, Key, RichText};
use engine::{Cmd, Event};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

const CJK_FONT: &str = "/System/Library/Fonts/Supplemental/Arial Unicode.ttf";

fn main() -> eframe::Result {
    env_logger::init();
    // panics otherwise vanish (no crash report for a Rust panic, no terminal when launched from Finder)
    let log = dirs::home_dir().map(|h| h.join("Library/Logs/stt.log"));
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(f) = log
            .as_ref()
            .and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok())
        {
            use std::io::Write;
            writeln!(
                &f,
                "{:?} {info}\n{}",
                std::time::SystemTime::now(),
                std::backtrace::Backtrace::force_capture()
            )
            .ok();
        }
        default_hook(info);
    }));
    if std::env::args().nth(1).as_deref() == Some("--transcribe") {
        return {
            cli_transcribe(&std::env::args().nth(2).expect("wav path"));
            Ok(())
        };
    }
    let opts = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([640.0, 96.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_visible(false)
            .with_title("SlimeTT"),
        ..Default::default()
    };
    eframe::run_native("stt", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

/// `stt --transcribe file.wav` — offline self-check of the ASR path without UI.
/// If running from an .app bundle, start a fresh copy of it ~1 s after this process exits. False when not bundled (cargo run).
fn relaunch_after_exit() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let Some(app) = exe
        .ancestors()
        .nth(3)
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
    else {
        return false;
    }; // SlimeTT.app/Contents/MacOS/stt
    std::process::Command::new("/bin/sh")
        .args(["-c", "sleep 1; /usr/bin/open \"$0\""])
        .arg(app)
        .spawn()
        .is_ok()
}

fn cli_transcribe(path: &str) {
    let s = Settings::load();
    let spec = asr::spec(&s.asr_model).expect("model key");
    let dir = asr::ensure(spec, &|d, t| eprint!("\r{} / {} MB", d >> 20, t >> 20)).unwrap();
    let rec = asr::Offline::load(spec, &dir, 8).unwrap();
    let mut r = hound::WavReader::open(path).unwrap();
    assert_eq!(r.spec().sample_rate, 16000, "need 16 kHz wav");
    let samples: Vec<f32> = r.samples::<i16>().map(|x| x.unwrap() as f32 / 32768.0).collect();
    let t = Instant::now();
    let raw = rec.transcribe(&samples);
    eprintln!("{} ms", t.elapsed().as_millis());
    println!("{}", text::finalize(&raw, &s.opencc));
}

/// 24-bit BMP of an egui screenshot over the popup's dark panel colour (no image encoder in the dependency tree).
fn save_bmp(path: &str, img: &egui::ColorImage) {
    let [w, h] = img.size;
    let row = (w * 3).div_ceil(4) * 4;
    let mut out = Vec::with_capacity(54 + row * h);
    out.extend(b"BM");
    out.extend(((54 + row * h) as u32).to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(54u32.to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend((w as i32).to_le_bytes());
    out.extend((h as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend([0u8; 24]);
    for y in (0..h).rev() {
        for c in &img.pixels[y * w..(y + 1) * w] {
            let over = |v: u8, bg: u32| (v as u32 + bg * (255 - c.a() as u32) / 255).min(255) as u8; // premultiplied over bg
            out.extend([over(c.b(), 46), over(c.g(), 30), over(c.r(), 30)]); // theme::BASE
        }
        out.resize(out.len() + row - w * 3, 0);
    }
    std::fs::write(path, out).ok();
}

/// Handle of an installed NSEvent monitor; dropping it stops the monitor.
type Monitor = objc2::rc::Retained<objc2::runtime::AnyObject>;

struct App {
    settings: Settings,
    engine: Sender<Cmd>,
    events: Receiver<Event>,
    popup: Option<macos::Popup>,
    hotkey_rx: Receiver<()>,
    hotkey_tx: Sender<()>,
    _hotkey_handles: Vec<Monitor>,
    /// Settings "Record" in progress: which slot (0/1), result channel, the capture monitor (dropped when done).
    esc_rx: Receiver<()>,
    _esc_monitor: Option<Monitor>,
    /// Child `fm serve` while the Apple backend is selected.
    fm: Option<std::process::Child>,
    fm_tried: Instant,
    capturing: Option<(usize, Receiver<Option<String>>, Option<Monitor>)>,
    _tray: Option<tray_icon::TrayIcon>,
    open_settings: Arc<AtomicBool>,
    quit: Arc<AtomicBool>,
    recording: bool,
    level: f32,
    segments: Vec<String>,
    provisional: String,
    llm_ok: bool,
    /// In-flight health check: (endpoint url it checked, result).
    health_rx: Option<Receiver<(String, bool)>>,
    last_health: Instant,
    /// The engine has answered the Stop sent by `finish`: no more text is coming.
    asr_done: bool,
    committing: Option<Instant>,
    /// On paste: the LLM rewriting the whole text, plus the raw text to paste if it fails.
    finalizing: Option<(String, Receiver<anyhow::Result<String>>)>,
    status: String,
    error: String,
    show_settings: bool,
    show_onboarding: bool,
    /// Accessibility was granted when the hotkey monitors were installed (monitors set up before the grant never fire).
    ax_armed: bool,
    llm_models: Vec<String>,
    /// In-flight /v1/models fetch: (url, ids); `models_url` = url last fetched, so each url is fetched once.
    models_rx: Option<Receiver<(String, Vec<String>)>>,
    models_url: String,
    input_devices: Vec<String>,
    target: Option<objc2::rc::Retained<objc2_app_kit::NSRunningApplication>>,
    t0: Instant,
    /// Set once the user clicks/types in the transcript: from then on it's plain text, new speech goes in at `caret`, no LLM rewrite.
    edited: Option<String>,
    /// What the box showed when the user started editing; diffed against the paste to learn vocabulary.
    edit_base: String,
    vocab: vocab::Vocab,
    vocab_new: (String, String),
    /// LLM verdicts on pending learned swaps: (heard, write, keep).
    vocab_tx: Sender<(String, String, bool)>,
    vocab_rx: Receiver<(String, String, bool)>,
    /// `STT_SHOT=out.bmp`: render the popup with sample text, save one frame there and exit (layout self-check).
    shot: Option<String>,
    caret: usize,
    /// We moved `caret` (speech insert); push it into egui's cursor next frame instead of reading egui's.
    caret_dirty: bool,
    slime_tex: Option<egui::TextureHandle>,
    last_voice: Instant,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        macos::hide_dock_icon();
        theme::apply(&cc.egui_ctx);
        if let Ok(bytes) = std::fs::read(CJK_FONT) {
            cc.egui_ctx.add_font(egui::epaint::text::FontInsert::new(
                "cjk",
                egui::FontData::from_owned(bytes),
                vec![
                    egui::epaint::text::InsertFontFamily {
                        family: egui::FontFamily::Proportional,
                        priority: egui::epaint::text::FontPriority::Lowest,
                    },
                    egui::epaint::text::InsertFontFamily {
                        family: egui::FontFamily::Monospace,
                        priority: egui::epaint::text::FontPriority::Lowest,
                    },
                ],
            ));
        } else {
            log::warn!("CJK font not found at {CJK_FONT}; Chinese will render as boxes");
        }
        let settings = Settings::load();
        let (etx, erx) = mpsc::channel();
        let engine = engine::spawn(settings.clone(), etx);
        let (hotkey_tx, hotkey_rx) = mpsc::channel();
        let (esc_tx, esc_rx) = mpsc::channel();
        let esc_monitor = {
            let ctx = cc.egui_ctx.clone();
            macos::on_local_esc(move || {
                esc_tx.send(()).ok();
                ctx.request_repaint();
            })
        };
        let popup = macos::Popup::from_handle(cc);
        let (vocab_tx, vocab_rx) = mpsc::channel();
        let open_settings = Arc::new(AtomicBool::new(false));
        let quit = Arc::new(AtomicBool::new(false));
        let tray = Self::make_tray(cc.egui_ctx.clone(), open_settings.clone(), quit.clone());
        let mut app = Self {
            settings,
            engine,
            events: erx,
            popup,
            hotkey_rx,
            hotkey_tx,
            _hotkey_handles: Vec::new(),
            fm: None,
            fm_tried: Instant::now() - Duration::from_secs(60),
            capturing: None,
            esc_rx,
            _esc_monitor: esc_monitor,
            _tray: tray,
            open_settings,
            quit,
            recording: false,
            level: 0.0,
            segments: Vec::new(),
            provisional: String::new(),
            llm_ok: false,
            health_rx: None,
            last_health: Instant::now() - Duration::from_secs(60),
            asr_done: true,
            committing: None,
            finalizing: None,
            edit_base: String::new(),
            vocab: vocab::Vocab::load(),
            vocab_new: Default::default(),
            vocab_tx,
            vocab_rx,
            shot: std::env::var("STT_SHOT").ok(),
            status: String::new(),
            error: String::new(),
            show_settings: false,
            show_onboarding: false,
            ax_armed: macos::accessibility_trusted(),
            llm_models: Vec::new(),
            models_rx: None,
            models_url: String::new(),
            input_devices: Vec::new(),
            target: None,
            t0: Instant::now(),
            edited: None,
            caret: 0,
            caret_dirty: false,
            slime_tex: None,
            last_voice: Instant::now() - Duration::from_secs(60),
        };
        app.install_hotkey(cc.egui_ctx.clone());
        if app.shot.is_some() {
            app.segments = vec![
                "今天我們來測試一下 dictation 的排版，".into(),
                "the quick brown fox jumps over the lazy dog,".into(),
                "看看三行文字跟史萊姆有沒有對齊，還有左右的邊距是不是一樣。".into(),
            ];
            match std::env::var("STT_SHOT_MOOD").as_deref() {
                Ok("write") => app.finalizing = Some((String::new(), mpsc::channel().1)),
                Ok("error") => {
                    app.segments.clear();
                    app.error = "Can't use the microphone: no input device found. Check Settings → Microphone.".into();
                }
                Ok("status") => {
                    app.segments.truncate(1);
                    app.error =
                        "Downloading the speech model: 120 / 300 MB. Dictation starts when this is done.".into();
                }
                _ => app.recording = true,
            }
        } else if !app.settings.onboarded || !app.ax_armed || macos::mic_status() != macos::Mic::Granted {
            app.show_onboarding = true;
            macos::activate_app();
        }
        app
    }

    fn make_tray(
        ctx: egui::Context,
        open_settings: Arc<AtomicBool>,
        quit: Arc<AtomicBool>,
    ) -> Option<tray_icon::TrayIcon> {
        use tray_icon::menu::{Menu, MenuEvent, MenuItem};
        let menu = Menu::new();
        let settings = MenuItem::new("Settings…", true, None);
        let quit_item = MenuItem::new("Quit SlimeTT", true, None);
        menu.append(&settings).ok()?;
        menu.append(&quit_item).ok()?;
        let (sid, qid) = (settings.id().clone(), quit_item.id().clone());
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if e.id == sid {
                open_settings.store(true, Ordering::Relaxed);
            } else if e.id == qid {
                quit.store(true, Ordering::Relaxed);
            }
            ctx.request_repaint();
        }));
        let mut b = tray_icon::TrayIconBuilder::new().with_menu(Box::new(menu));
        // template image: macOS tints it for light and dark menu bars
        if let Some(i) = blob::tray_icon() {
            b = b.with_icon_templated(i);
        } else {
            b = b.with_title("🎙");
        }
        b.with_tooltip("SlimeTT dictation")
            .build()
            .map_err(|e| log::error!("tray: {e}"))
            .ok()
    }

    fn install_hotkey(&mut self, ctx: egui::Context) {
        self._hotkey_handles.clear();
        for name in [&self.settings.hotkey, &self.settings.hotkey2]
            .into_iter()
            .filter(|h| !h.is_empty())
        {
            let Some(hk) = macos::parse_hotkey(name) else {
                self.error = format!("The hotkey {name:?} isn't valid. Pick another one in Settings.");
                continue;
            };
            let (tx, ctx) = (self.hotkey_tx.clone(), ctx.clone());
            self._hotkey_handles.extend(macos::install_hotkey(hk, move || {
                tx.send(()).ok();
                ctx.request_repaint();
            }));
        }
    }

    /// Show only the Settings window (not the popup).
    fn open_settings_window(&mut self, ctx: &egui::Context) {
        self.show_settings = true;
        macos::activate_app();
        ctx.request_repaint();
    }

    /// (url, model) of the selected LLM backend.
    fn llm_endpoint(&self) -> (String, String) {
        if self.settings.llm_backend == "apple" {
            (llm::FM_URL.into(), llm::FM_MODEL.into())
        } else {
            (self.settings.llm_url.clone(), self.settings.llm_model.clone())
        }
    }

    fn open_popup(&mut self) {
        if let Some(p) = &self.popup {
            if !p.visible() {
                self.target = macos::frontmost_app();
            }
            p.show(self.settings.popup_position == "top");
        }
    }

    fn toggle_recording(&mut self) {
        if self.committing.is_some() {
            return;
        }
        if self.recording {
            return self.finish();
        }
        if !self.popup.as_ref().is_some_and(|p| p.visible()) {
            self.open_popup();
        }
        self.recording = true;
        self.engine.send(Cmd::Start).ok();
        if self.settings.sounds {
            macos::play_sound("Tink");
        }
        self.error.clear();
    }

    /// Hotkey again or Enter: stop the mic, then (in `poll`) wait for the last words, clean up, close and paste.
    fn finish(&mut self) {
        if self.committing.is_some() {
            return;
        }
        self.recording = false;
        self.asr_done = false;
        self.engine.send(Cmd::Stop).ok(); // also when not recording: its Stopped reply is what `poll` waits for
        if self.settings.sounds {
            macos::play_sound("Pop");
        }
        self.committing = Some(Instant::now());
    }

    fn cancel(&mut self) {
        if self.recording {
            self.recording = false;
            self.engine.send(Cmd::Stop).ok();
        }
        self.segments.clear();
        self.provisional.clear();
        self.committing = None;
        self.finalizing = None;
        self.edited = None;
        if let Some(p) = &self.popup {
            p.hide(self.target.take().as_deref());
        }
    }

    fn display_text(&self) -> String {
        // trailing space: the cursor sits ready for the next word, and Backspace starts on the last one
        let mut t = text::join(&self.segments);
        if !t.is_empty() && !t.ends_with('\n') {
            t.push(' ');
        }
        t
    }

    fn push_final(&mut self, seg: String) {
        let seg = self.vocab.apply(&text::finalize(&seg, &self.settings.opencc));
        if let Some(buf) = &mut self.edited {
            match text::instant_command(&seg) {
                Some(text::Instant::Undo) => {} // shortcut: no undo after manual edits, track inserts if wanted
                Some(text::Instant::Clear) => {
                    buf.clear();
                    self.caret = 0;
                    self.edit_base.clear();
                }
                None => {
                    // learn from what was typed so far, then restart the comparison after the insert: words dictated
                    // over a deleted phrase are new speech, not a spelling fix (that is how "to → do" got learned)
                    let (base, cur) = (std::mem::take(&mut self.edit_base), buf.clone());
                    self.learn_fixes(&base, &cur);
                    let buf = self.edited.as_mut().expect("checked above");
                    self.caret = text::insert_at(buf, self.caret, &seg);
                    if self.caret == buf.chars().count() {
                        buf.push(' ');
                        self.caret += 1;
                    }
                    self.edit_base = buf.clone();
                    self.caret_dirty = true;
                }
            }
            return;
        }
        match text::instant_command(&seg) {
            Some(text::Instant::Undo) => {
                self.segments.pop();
            }
            Some(text::Instant::Clear) => self.segments.clear(),
            None => self.segments.push(seg),
        }
        self.caret = self.display_text().chars().count(); // cursor follows the text to its end
        self.caret_dirty = true;
    }

    /// Count the typed word swaps between `shown` and `now`. A new swap is only applied to future dictation once
    /// the LLM judges it a real mishearing rather than a one-off edit (without an LLM: once seen three times).
    fn learn_fixes(&mut self, shown: &str, now: &str) {
        let cc = &self.settings.opencc;
        let (shown, now) = (text::finalize(shown, cc), text::finalize(now, cc));
        let before = self.vocab.clone();
        for e in self.vocab.learn(&shown, &now) {
            if self.settings.llm_enabled && self.llm_ok {
                let ((url, model), tx, (s, n)) =
                    (self.llm_endpoint(), self.vocab_tx.clone(), (shown.clone(), now.clone()));
                std::thread::spawn(
                    move || match llm::keep_fix(&url, &model, &e.heard, &e.write, e.seen, &s, &n) {
                        Ok(keep) => {
                            log::info!("vocab: {:?} -> {:?} seen {}: keep={keep}", e.heard, e.write, e.seen);
                            tx.send((e.heard, e.write, keep)).ok();
                        }
                        Err(err) => log::warn!("vocab llm: {err:#}"),
                    },
                );
            } else if e.seen >= 3 {
                self.vocab.confirm(&e.heard, &e.write);
            }
        }
        if self.vocab != before {
            self.vocab.save();
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((heard, write, keep)) = self.vocab_rx.try_recv() {
            if keep {
                self.vocab.confirm(&heard, &write);
                self.vocab.save();
            }
        }
        // Global key monitors only see other apps' keys if Accessibility was granted before this process started,
        // so restart once when it gets granted while running.
        if !self.ax_armed && macos::accessibility_trusted() {
            self.ax_armed = true;
            if relaunch_after_exit() {
                self.quit.store(true, Ordering::Relaxed);
            } else {
                self.install_hotkey(ctx.clone());
            }
        }
        while self.hotkey_rx.try_recv().is_ok() {
            self.toggle_recording();
        }
        // Esc only cancels when the popup is the focused window (not e.g. the Settings window)
        while self.esc_rx.try_recv().is_ok() {
            if self.popup.as_ref().is_some_and(|p| p.is_key()) {
                self.cancel();
            }
        }
        if self.open_settings.swap(false, Ordering::Relaxed) {
            self.open_settings_window(ctx);
        }
        // keep `fm serve` running exactly while the Apple backend is selected
        let want_fm = self.settings.llm_enabled && self.settings.llm_backend == "apple";
        // (rate-limited and only while unreachable: a leftover server holding the port makes a new one exit at once)
        if want_fm
            && !self.llm_ok
            && self.fm_tried.elapsed() > Duration::from_secs(10)
            && self
                .fm
                .as_mut()
                .is_none_or(|c| c.try_wait().map_or(true, |s| s.is_some()))
        {
            self.fm = llm::spawn_fm();
            self.fm_tried = Instant::now();
            self.last_health = Instant::now() - Duration::from_secs(9); // re-check in ~1 s, once it has started
        } else if !want_fm && let Some(mut c) = self.fm.take() {
            c.kill().ok();
            c.wait().ok();
        }
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                Event::Level(l) => {
                    self.level = l;
                    if l > 0.12 {
                        self.last_voice = Instant::now();
                    }
                }
                Event::Provisional(t) => {
                    self.provisional = self.vocab.apply(&text::finalize(&t, &self.settings.opencc))
                }
                Event::Final(t) => self.push_final(t),
                Event::Status(s) => self.status = s,
                Event::Error(e) => {
                    self.error = e;
                    self.recording = false;
                }
                Event::Stopped => self.asr_done = true,
            }
        }
        // LLM health, every 10 s, off-thread
        if let Some(Ok((url, ok))) = self.health_rx.as_ref().map(|rx| rx.try_recv()) {
            self.health_rx = None;
            // a result for an endpoint that's no longer selected (switched/edited meanwhile) is stale: re-check now
            if url == self.llm_endpoint().0 {
                self.llm_ok = ok;
            } else {
                self.last_health = Instant::now() - Duration::from_secs(60);
            }
        }
        if self.health_rx.is_none() && self.last_health.elapsed() > Duration::from_secs(10) {
            let (tx, rx) = mpsc::channel();
            let url = self.llm_endpoint().0;
            std::thread::spawn(move || {
                let ok = llm::healthy(&url);
                tx.send((url, ok)).ok();
            });
            self.health_rx = Some(rx);
            self.last_health = Instant::now();
        }
        if let Some(t) = self.committing {
            if let Some((raw, rx)) = self.finalizing.take() {
                match rx.try_recv() {
                    Ok(Ok(out)) => self.commit(out),
                    Err(mpsc::TryRecvError::Empty) => self.finalizing = Some((raw, rx)), // not ready: keep waiting (the request itself gives up after 30 s)
                    r => {
                        if let Ok(Err(e)) = r {
                            log::warn!("llm: {e:#}");
                            self.llm_ok = false;
                        }
                        self.commit(raw)
                    } // failed: paste raw
                }
            } else if self.asr_done || t.elapsed() > Duration::from_secs(3) {
                // 3 s: engine busy loading a model, don't hang
                // regenerate the whole text in one LLM pass right before pasting
                let raw = self.edited.clone().unwrap_or_else(|| text::join(&self.segments));
                if self.settings.llm_enabled && self.llm_ok && !raw.trim().is_empty() {
                    let (tx, rx) = mpsc::channel();
                    let ((url, model), polish, voc, r) = (
                        self.llm_endpoint(),
                        self.settings.polish,
                        self.vocab.prompt(),
                        raw.clone(),
                    );
                    std::thread::spawn(move || {
                        tx.send(llm::cleanup(&url, &model, polish, &voc, &r)).ok();
                    });
                    self.finalizing = Some((raw, rx));
                } else {
                    self.commit(raw);
                }
            }
        }
        if self.quit.load(Ordering::Relaxed) {
            self.engine.send(Cmd::Quit).ok();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn commit(&mut self, raw: String) {
        self.committing = None;
        self.finalizing = None;
        if let Some(e) = self.edited.take() {
            let base = std::mem::take(&mut self.edit_base);
            self.learn_fixes(&base, &e);
        }
        let out = text::finalize(&raw, &self.settings.opencc);
        let target = self.target.take();
        if let Some(p) = &self.popup {
            p.hide(target.as_deref());
        }
        self.segments.clear();
        self.provisional.clear();
        if out.trim().is_empty() {
            return;
        }
        if !macos::accessibility_trusted() {
            self.error =
                "SlimeTT needs the Accessibility permission to paste. Turn it on in System Settings, then try again."
                    .into();
            macos::open_accessibility_settings();
            return;
        }
        let sounds = self.settings.sounds;
        std::thread::spawn(move || {
            if sounds {
                macos::play_sound("Bottle");
            }
            macos::paste_text(&(out.trim_end_matches(' ').to_string() + " ")); // trailing space: the next dictation or typing continues cleanly
        });
    }

    fn popup_ui(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let mut editor_focused = false;
        let glass = self.popup.as_ref().is_some_and(|p| p.1);
        let fill = if glass {
            Color32::TRANSPARENT
        } else {
            theme::BASE.gamma_multiply(0.92)
        };
        let frame = egui::Frame::new().fill(fill).corner_radius(16.0).inner_margin(12.0);
        egui::CentralPanel::default().frame(frame).show(root, |ui| {
            // whole popup is ~3 text lines tall: left = slime centred on the text field, right = 3-line text field
            // (errors and model download/loading progress are shown inside the field, above the transcript)
            let font = egui::FontId::proportional(18.0);
            let three_lines = ui.available_height(); // window height is chosen to fit ~3 lines
            ui.horizontal_top(|ui| {
                let slime = three_lines * 0.8;
                ui.allocate_ui_with_layout(
                    egui::vec2(slime * blob::aspect(), three_lines),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.add_space((three_lines - slime) / 2.0); // centered on the text field
                        // hold briefly so the bubble doesn't flicker between words
                        let mood = if self.finalizing.is_some() {
                            blob::Mood::Writing
                        } else if self.recording && self.last_voice.elapsed() < Duration::from_millis(600) {
                            blob::Mood::Talking
                        } else {
                            blob::Mood::Idle
                        };
                        blob::show(ui, &mut self.slime_tex, slime, self.t0.elapsed().as_secs_f32(), mood);
                    },
                );
                ui.add_space(1.0); // the pencil's eraser end leans ~8pt past the slime at the right end of its line
                ui.vertical(|ui| {
                    // a text row leaves more empty space under the glyphs than over them; push the field down so three
                    // lines of ink sit centred in the popup (and on the slime). Checked with STT_SHOT.
                    const INK_SHIFT: f32 = 4.0;
                    ui.add_space(INK_SHIFT);
                    let three_lines = three_lines - INK_SHIFT;
                    egui::ScrollArea::vertical()
                        .min_scrolled_height(three_lines)
                        .max_height(three_lines)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let msg = if !self.error.is_empty() {
                                Some((self.error.clone(), theme::RED))
                            } else if !self.status.is_empty() {
                                Some((self.status.clone(), theme::BLUE))
                            } else {
                                None
                            };
                            if let Some((m, col)) = &msg {
                                // no bold face is loaded: draw the text twice, a hair apart
                                let g = ui.painter().layout(
                                    m.clone(),
                                    egui::FontId::proportional(16.0),
                                    *col,
                                    ui.available_width(),
                                );
                                let at = ui
                                    .allocate_exact_size(g.size() + egui::vec2(0.0, 2.0), egui::Sense::hover())
                                    .0
                                    .min
                                    + egui::vec2(0.0, 2.0); // lines up with the transcript's first row
                                ui.painter().galley(at, g.clone(), *col);
                                ui.painter().galley(at + egui::vec2(0.6, 0.0), g, *col);
                            }
                            let hint = if self.settings.llm_enabled && !self.llm_ok {
                                "Cleanup isn't available right now — your words will be pasted as spoken."
                            } else {
                                ""
                            };
                            let mut buf = self.edited.clone().unwrap_or_else(|| self.display_text());
                            let out = egui::TextEdit::multiline(&mut buf)
                                .id_salt("transcript")
                                .font(font)
                                .hint_text(RichText::new(hint).size(14.0))
                                .text_color(theme::TEXT)
                                .frame(egui::Frame::NONE)
                                .desired_width(f32::INFINITY)
                                .desired_rows(if msg.is_some() { 1 } else { 3 })
                                .return_key(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, Key::Enter))
                                .interactive(self.committing.is_none())
                                .show(ui); // locked while finishing: edits made now would be lost
                            editor_focused = out.response.has_focus();
                            // keep the keyboard in the transcript so Backspace/typing work without clicking first
                            if !editor_focused
                                && self.committing.is_none()
                                && self.popup.as_ref().is_some_and(|p| p.visible())
                            {
                                out.response.request_focus();
                            }
                            if self.caret_dirty {
                                let mut st = out.state;
                                st.cursor.set_char_range(Some(egui::text_selection::CCursorRange::one(
                                    egui::text::CCursor::new(self.caret),
                                )));
                                st.store(ui.ctx(), out.response.id);
                                self.caret_dirty = false;
                            } else if out.response.clicked() || out.response.changed() || self.edited.is_some() {
                                if let Some(r) = out.cursor_range {
                                    self.caret = r.primary.index.into();
                                }
                                if self.edited.is_none() {
                                    self.edit_base = self.display_text();
                                }
                                self.edited = Some(buf);
                            }
                            if !self.provisional.is_empty() {
                                ui.label(RichText::new(&self.provisional).size(18.0).color(theme::OVERLAY));
                            }
                        });
                });
            });
        });
        let (enter, shift) = ctx.input(|i| (i.key_pressed(Key::Enter), i.modifiers.shift));
        if enter && shift {
            if editor_focused {
            }
            // the TextEdit already inserted it
            else if let Some(b) = &mut self.edited {
                self.caret = text::insert_at(b, self.caret, "\n");
                self.caret_dirty = true;
            } else {
                self.segments.push("\n".into());
            }
        } else if enter {
            self.finish();
        }
    }

    /// First-launch welcome + permissions (also shown at launch whenever a permission is missing).
    fn onboarding_ui(&mut self, ctx: egui::Context) {
        let ax = macos::accessibility_trusted();
        let mic = macos::mic_status();
        let (mut open, mut done) = (true, false);
        let hotkeys = [&self.settings.hotkey, &self.settings.hotkey2]
            .into_iter()
            .filter(|h| !h.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" or ");
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("onboarding"),
            egui::ViewportBuilder::default().with_title("Welcome to SlimeTT").with_inner_size([460.0, 400.0]).with_resizable(false), |root, _| {
            egui::CentralPanel::default().show(root, |ui| {
                ui.heading("Welcome to SlimeTT");
                ui.label("Press your hotkey, talk, press it again — the text is pasted where your cursor was. Everything runs on this Mac.");
                ui.add_space(14.0);
                let step = |ui: &mut egui::Ui, title: &str, ok: bool, why: &str| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(if ok { "✓" } else { "○" }).size(18.0).color(if ok { theme::GREEN } else { theme::SUBTEXT }));
                        ui.label(RichText::new(title).strong().size(15.0));
                    });
                    ui.label(RichText::new(why).color(theme::SUBTEXT));
                };
                step(ui, "1. Microphone", mic == macos::Mic::Granted, "So SlimeTT can hear you while you dictate.");
                match mic {
                    macos::Mic::NotAsked => if ui.button("Allow microphone").clicked() { macos::request_mic(); },
                    macos::Mic::Denied => if ui.button("Open Microphone settings").clicked() { macos::open_microphone_settings(); },
                    macos::Mic::Granted => {}
                }
                ui.add_space(12.0);
                step(ui, "2. Accessibility", ax, "So the hotkey works in every app and the text can be pasted at your cursor.");
                if !ax {
                    if ui.button("Allow accessibility").clicked() { macos::prompt_accessibility(); }
                    ui.label(RichText::new("Turn SlimeTT on in System Settings — it then restarts itself once so the hotkey works.").size(11.0).color(theme::SUBTEXT));
                }
                ui.add_space(12.0);
                step(ui, "3. Your hotkey", true, &format!("Tap {hotkeys} to start, tap again to paste. Esc cancels. Change it in Settings (menu bar slime)."));
                ui.add_space(16.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = ax && mic == macos::Mic::Granted;
                    if ui.add_enabled(ready, egui::Button::new(RichText::new("Start dictating").strong())).clicked() { done = true; }
                    if !ready && ui.link("Skip for now").clicked() { done = true; }
                });
            });
            if root.ctx().input(|i| i.viewport().close_requested()) { open = false; }
        });
        ctx.request_repaint_after(Duration::from_millis(500)); // pick up permission changes made in System Settings
        if done || !open {
            self.show_onboarding = false;
            if !self.settings.onboarded {
                self.settings.onboarded = true;
                self.settings.save();
            }
        }
    }

    fn settings_ui(&mut self, ctx: egui::Context) {
        // model list for the OpenAI-compatible endpoint: off the UI thread, once per url
        if let Some(Ok((url, ids))) = self.models_rx.as_ref().map(|rx| rx.try_recv()) {
            self.models_rx = None;
            if url == self.settings.llm_url {
                self.llm_models = ids;
            }
        }
        if self.settings.llm_backend == "openai"
            && self.llm_ok
            && self.models_rx.is_none()
            && self.models_url != self.settings.llm_url
        {
            let (tx, rx) = mpsc::channel();
            let url = self.settings.llm_url.clone();
            std::thread::spawn(move || {
                let ids = llm::models(&url);
                tx.send((url, ids)).ok();
            });
            self.models_rx = Some(rx);
            self.models_url = self.settings.llm_url.clone();
        }
        if self.input_devices.is_empty() {
            self.input_devices = audio::devices();
        }
        let before = self.settings.clone();
        let mut open = true;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("settings"),
            egui::ViewportBuilder::default().with_title("SlimeTT Settings").with_inner_size([460.0, 560.0]), |root, _| {
            egui::CentralPanel::default().show(root, |ui| { egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("g").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    for (slot, label) in [(0, "Hotkey"), (1, "Second hotkey")] {
                        ui.label(label);
                        ui.horizontal(|ui| {
                            let cur = if slot == 0 { &mut self.settings.hotkey } else { &mut self.settings.hotkey2 };
                            if self.capturing.as_ref().is_some_and(|c| c.0 == slot) {
                                ui.label(RichText::new("Press a key or tap a modifier… (Esc cancels)").color(theme::BLUE));
                            } else {
                                ui.label(RichText::new(if cur.is_empty() { "none" } else { cur.as_str() }).monospace().strong());
                                if self.capturing.is_none() && ui.button("Record").clicked() {
                                    self._hotkey_handles.clear(); // so the tap being recorded doesn't start dictation
                                    let (tx, rx) = mpsc::channel();
                                    let ctx = ui.ctx().clone();
                                    let handle = macos::capture_hotkey(move |r| { tx.send(r).ok(); ctx.request_repaint(); });
                                    self.capturing = Some((slot, rx, handle));
                                }
                                if slot == 1 && !cur.is_empty() && ui.button("Clear").clicked() { cur.clear(); }
                            }
                        });
                        ui.end_row();
                    }
                    let s = &mut self.settings;
                    ui.label("ASR model");
                    egui::ComboBox::from_id_salt("asr").selected_text(asr::spec(&s.asr_model).map(|m| m.label).unwrap_or("?")).show_ui(ui, |ui| {
                        for m in asr::CATALOG.iter().filter(|m| m.kind != asr::Kind::OnlineTransducer) { ui.selectable_value(&mut s.asr_model, m.key.into(), m.label); }
                    }); ui.end_row();
                    ui.label("Streaming model");
                    egui::ComboBox::from_id_salt("stream").selected_text(asr::spec(&s.asr_streaming_model).map(|m| m.label).unwrap_or("none (rolling re-transcription)")).show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.asr_streaming_model, String::new(), "none (rolling re-transcription)");
                        for m in asr::CATALOG.iter().filter(|m| m.kind == asr::Kind::OnlineTransducer) { ui.selectable_value(&mut s.asr_streaming_model, m.key.into(), m.label); }
                    }); ui.end_row();
                    ui.label("Microphone");
                    egui::ComboBox::from_id_salt("mic").selected_text(if s.input_device.is_empty() { "System default" } else { &s.input_device }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.input_device, String::new(), "System default").on_hover_text("Uses the built-in mic when the default is Bluetooth (AirPods), so your audio doesn't drop to call quality");
                        for d in &self.input_devices { ui.selectable_value(&mut s.input_device, d.clone(), d); }
                    }); ui.end_row();
                    ui.label("Traditional Chinese");
                    egui::ComboBox::from_id_salt("cc").selected_text(&s.opencc).show_ui(ui, |ui| {
                        for (k, l) in [("s2twp", "s2twp (Taiwan words)"), ("s2tw", "s2tw (Taiwan chars)"), ("s2hk", "s2hk (Hong Kong)"), ("off", "off")] { ui.selectable_value(&mut s.opencc, k.into(), l); }
                    }); ui.end_row();
                    ui.label("Popup position");
                    egui::ComboBox::from_id_salt("pos").selected_text(&s.popup_position).show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.popup_position, "bottom".into(), "bottom"); ui.selectable_value(&mut s.popup_position, "top".into(), "top");
                    }); ui.end_row();
                    ui.label("Sound effects"); ui.checkbox(&mut s.sounds, ""); ui.end_row();
                });
                ui.separator();
                ui.heading("LLM cleanup");
                let down = if self.settings.llm_backend == "apple" { "Status: Apple Foundation Models unavailable (needs macOS 26+ with Apple Intelligence on) — dictation works, cleanup is skipped" }
                           else { "Status: endpoint not reachable — dictation works, cleanup is skipped" };
                ui.label(RichText::new(if self.llm_ok { "Status: connected" } else { down }).color(if self.llm_ok { theme::GREEN } else { theme::RED }));
                let s = &mut self.settings;
                egui::Grid::new("g2").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Enabled"); ui.checkbox(&mut s.llm_enabled, ""); ui.end_row();
                    ui.label("Provider");
                    egui::ComboBox::from_id_salt("backend").selected_text(if s.llm_backend == "apple" { "Apple Foundation Models (on-device)" } else { "OpenAI-compatible endpoint" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.llm_backend, "apple".into(), "Apple Foundation Models (on-device)");
                        ui.selectable_value(&mut s.llm_backend, "openai".into(), "OpenAI-compatible endpoint");
                    }); ui.end_row();
                    if s.llm_backend == "openai" {
                        ui.label("URL"); ui.text_edit_singleline(&mut s.llm_url).on_hover_text("Base URL, e.g. http://127.0.0.1:11434 (oMLX, Ollama, LM Studio…)"); ui.end_row();
                        ui.label("Model");
                        egui::ComboBox::from_id_salt("llm").selected_text(&s.llm_model).show_ui(ui, |ui| {
                            for m in &self.llm_models { ui.selectable_value(&mut s.llm_model, m.clone(), m); }
                        }); ui.end_row();
                    }
                    ui.label("Polish wording"); ui.checkbox(&mut s.polish, "").on_hover_text("Also tighten rambling phrasing (may change wording)"); ui.end_row();
                });
                ui.separator();
                ui.heading("Vocabulary");
                ui.label(RichText::new("Fixes you type in the popup are counted here. Left: what it hears, right: what to write. A fix is only applied once it looks like a real mishearing (\"maybe\" rows are not applied yet — Keep to apply now). Leave left empty to just teach the LLM a name.").size(11.0).color(theme::SUBTEXT));
                let before_vocab = self.vocab.clone();
                egui::Grid::new("vocab").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                    let mut remove = None;
                    for (i, e) in self.vocab.entries.iter_mut().enumerate() {
                        ui.add(egui::TextEdit::singleline(&mut e.heard).desired_width(150.0));
                        ui.add(egui::TextEdit::singleline(&mut e.write).desired_width(150.0));
                        if ui.button("✕").clicked() { remove = Some(i); }
                        if e.pending {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("maybe ({}×)", e.seen)).size(11.0).color(theme::SUBTEXT));
                                if ui.small_button("Keep").clicked() { e.pending = false; }
                            });
                        }
                        ui.end_row();
                    }
                    if let Some(i) = remove { self.vocab.entries.remove(i); }
                    let (h, w) = &mut self.vocab_new;
                    ui.add(egui::TextEdit::singleline(h).hint_text("heard").desired_width(150.0));
                    ui.add(egui::TextEdit::singleline(w).hint_text("write").desired_width(150.0));
                    if ui.add_enabled(!w.trim().is_empty(), egui::Button::new("Add")).clicked() {
                        let (h, w) = std::mem::take(&mut self.vocab_new);
                        self.vocab.entries.push(vocab::Entry { heard: h.trim().into(), write: w.trim().into(), ..Default::default() });
                    }
                    ui.end_row();
                });
                if self.vocab != before_vocab { self.vocab.save(); }
                ui.separator();
                ui.heading("Permissions");
                let ax = macos::accessibility_trusted();
                ui.label(RichText::new(if ax { "Accessibility: granted" } else { "Accessibility: NOT granted — needed to paste into other apps and for the hotkey" }).color(if ax { theme::GREEN } else { theme::RED }));
                if !ax && ui.button("Open Accessibility settings").clicked() { macos::open_accessibility_settings(); }
                ui.label(RichText::new("Microphone access is requested the first time you record.").size(11.0).color(theme::SUBTEXT));
                ui.separator();
                ui.label(RichText::new(format!("Models: {}", config::models_dir().display())).size(11.0).color(theme::SUBTEXT));
            }); });
            if root.ctx().input(|i| i.viewport().close_requested()) { open = false; }
        });
        if let Some(r) = self.capturing.as_ref().and_then(|c| c.1.try_recv().ok()) {
            let slot = self.capturing.take().unwrap().0;
            if let Some(name) = r {
                *(if slot == 0 {
                    &mut self.settings.hotkey
                } else {
                    &mut self.settings.hotkey2
                }) = name;
            }
            self.install_hotkey(ctx.clone()); // re-arm (also when cancelled)
        }
        if !open {
            self.show_settings = false;
            if self.capturing.take().is_some() {
                self.install_hotkey(ctx.clone());
            }
        }
        if self.settings != before {
            self.settings.save();
            self.engine.send(Cmd::Reload(Box::new(self.settings.clone()))).ok();
            if (&self.settings.hotkey, &self.settings.hotkey2) != (&before.hotkey, &before.hotkey2) {
                self.install_hotkey(ctx);
            }
            if (&self.settings.llm_url, &self.settings.llm_backend) != (&before.llm_url, &before.llm_backend) {
                self.llm_models.clear();
                self.models_url.clear();
                self.llm_ok = false;
                self.last_health = Instant::now() - Duration::from_secs(60);
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(mut c) = self.fm.take() {
            c.kill().ok();
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.poll(ctx);
        if self.shot.is_some() {
            self.last_voice = Instant::now(); // keep the speech bubble up
            if self.t0.elapsed() > Duration::from_secs(2) && self.shot.as_ref().is_some_and(|p| !p.is_empty()) {
                let path = self.shot.replace(String::new()).unwrap(); // Some(""): requested, keep the sample state until captured
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(
                    move |img| {
                        save_bmp(&path, &img);
                        std::process::exit(0);
                    },
                )));
            }
        }
        // Settings is drawn from the root (popup) viewport's ui(), which eframe skips while the popup is hidden.
        // Keep asking for hidden paints while it's open; one isn't enough since a new window stays hidden until drawn again.
        ctx.request_repaint_after(if self.recording || self.committing.is_some() {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(250)
        });
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.popup_ui(ui);
        if self.show_settings {
            self.settings_ui(ui.ctx().clone());
        }
        if self.show_onboarding {
            self.onboarding_ui(ui.ctx().clone());
        }
    }
}
