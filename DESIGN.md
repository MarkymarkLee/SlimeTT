# SlimeTT (STT) — local zh-TW/en dictation for macOS

Settled decisions (grill session, 2026-11-03).

## Product
- Hotkey (default: tap Right Option, configurable) toggles recording: press to start, press to stop.
- Floating popup (bottom-center by default): Siri-style amplitude blob + text buffer. Provisional text gray, finalized black.
- Each pause-finalized segment appends to the buffer. Multiple start/stop cycles append to the same buffer.
- Enter commits the buffer into the previously focused app via clipboard paste (save → set → Cmd+V → restore). Shift+Enter = newline in buffer. Esc = cancel.
- Languages: Traditional Chinese (Taiwan) + English, code-switched mid-sentence. OpenCC `s2twp` is always the final step (settings: s2twp / s2hk / off).
- Numbers are written numerically.
- Menu bar icon (no Dock icon) with Settings / Quit. Sound effects via macOS system sounds (start / stop / commit), toggle in settings.

## ASR (in-process)
- Engine: sherpa-onnx via `sherpa-rs`. Models download on first launch to `~/Library/Application Support/stt/models/`.
- Live text: rolling re-transcription of the current segment every ~0.7s with the chosen non-streaming model, if its latency on a 10s clip is ≤400ms. Otherwise two-tier (streaming model for gray text, non-streaming re-transcribes on finalize).
- Segment finalized on ~600ms silence (Silero VAD) or hard cap at 15s.
- Audio: default input device (selectable), 16kHz mono via cpal.
- Model chosen by eval (eval/): Qwen3-ASR-0.6B, FireRedASR-AED, FireRedASR-CTC, SenseVoice-small, FunASR-Nano, Whisper-large-v3-turbo, X-ASR-zh-en, Zipformer multilingual 2025-02. Parakeet excluded (no Chinese). Whisper only gets a whisper.cpp/Metal path if it wins.
- Metrics: CER (zh), WER (en), traditional-character rate pre-OpenCC, latency per 10s clip, first-word latency for streaming models.

## LLM pass (optional)
- Provider (Settings): Apple Foundation Models on-device (default; the app runs `/usr/bin/fm serve` on 127.0.0.1:11976 while selected) or any OpenAI-compatible endpoint (URL + model, e.g. oMLX). Same chat-completions client for both.
- One pass after the user stops (hotkey again or Enter): send the whole raw buffer, paste the cleaned result. The popup shows raw text while talking; nothing is swapped in live.
- Scope (default "strict"): punctuation (full-width zh / half-width en), apply spoken self-corrections ("no wait, I mean…", "刪掉"), honor "new line / 換行 / new paragraph", fix obvious homophone errors, numerals. "Polish" toggle rewrites for clarity.
- Vocabulary (vocab.toml): short word swaps the user makes in the popup are learned (token diff of shown vs pasted text). Exact replacement on every segment; the list goes into the LLM system prompt as preferred spellings for sound-alikes. Editable in Settings.
- Hard-coded instant commands bypass the LLM: undo/復原 (drop last segment), clear/清除.
- Model: small (~4B-class 4-bit) chosen by eval against Qwen3.6-35B-A3B reference; selectable. Thinking off.
- If the LLM is down: app works, popup shows "No cleanup" label, settings shows status; re-check /health every 10s. OpenCC + numerals still run.

## UI / tech
- Rust. eframe/egui on wgpu; blob is a custom wgpu paint callback. Bundled Noto Sans TC.
- Small Swift/ObjC bridge only where macOS APIs are painful from Rust (permissions, paste keystroke, caret-independent panel behaviour).
- Settings (TOML in Application Support): hotkey, ASR model, LLM on/off + URL + model, polish toggle, OpenCC variant, popup position, input device, sound effects.
- Permissions: request Microphone on first launch; open Accessibility pane with instructions; refuse commit until granted.
