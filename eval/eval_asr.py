"""Run every candidate ASR model over clips/ and write results.md + results.json.

  .venv/bin/python eval_asr.py [model-name ...]   # default: all models whose files are present
"""
import glob, json, pathlib, sys, time, soundfile as sf, numpy as np, sherpa_onnx
from metrics import mer, trad_rate

HERE = pathlib.Path(__file__).parent
M = HERE / "models"
THREADS = 4

def g(d, pat):
    hits = sorted(glob.glob(str(M / d / pat)))
    if not hits:
        raise FileNotFoundError(f"{d}/{pat}")
    return hits[0]

# name -> (kind, builder). kind: "offline" or "online" (streaming).
MODELS = {
    "sense-voice-int8": ("offline", lambda: sherpa_onnx.OfflineRecognizer.from_sense_voice(
        model=g("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09", "model.int8.onnx"),
        tokens=g("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09", "tokens.txt"),
        language="auto", use_itn=True, num_threads=THREADS)),
    "sense-voice-fp32": ("offline", lambda: sherpa_onnx.OfflineRecognizer.from_sense_voice(
        model=g("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2025-09-09", "model.onnx"),
        tokens=g("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2025-09-09", "tokens.txt"),
        language="auto", use_itn=True, num_threads=THREADS)),
    "funasr-nano-int8": ("offline", lambda: sherpa_onnx.OfflineRecognizer.from_funasr_nano(
        encoder_adaptor=g("sherpa-onnx-funasr-nano-int8-2025-12-30", "encoder*.onnx"),
        llm=g("sherpa-onnx-funasr-nano-int8-2025-12-30", "llm*.onnx"),
        embedding=g("sherpa-onnx-funasr-nano-int8-2025-12-30", "embedding*.onnx"),
        tokenizer=g("sherpa-onnx-funasr-nano-int8-2025-12-30", "Qwen3*"),
        num_threads=THREADS)),
    "qwen3-asr-0.6b-int8": ("offline", lambda: sherpa_onnx.OfflineRecognizer.from_qwen3_asr(
        conv_frontend=g("sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25", "conv_frontend.onnx"),
        encoder=g("sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25", "encoder.int8.onnx"),
        decoder=g("sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25", "decoder.int8.onnx"),
        tokenizer=g("sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25", "tokenizer"),
        num_threads=THREADS)),
    "fire-red-asr2-int8": ("offline", lambda: sherpa_onnx.OfflineRecognizer.from_fire_red_asr(
        encoder=g("sherpa-onnx-fire-red-asr2-zh_en-int8-2026-02-26", "encoder.int8.onnx"),
        decoder=g("sherpa-onnx-fire-red-asr2-zh_en-int8-2026-02-26", "decoder.int8.onnx"),
        tokens=g("sherpa-onnx-fire-red-asr2-zh_en-int8-2026-02-26", "tokens.txt"),
        num_threads=THREADS)),
    "fire-red-asr2-ctc-int8": ("offline", lambda: sherpa_onnx.OfflineRecognizer.from_fire_red_asr_ctc(
        model=g("sherpa-onnx-fire-red-asr2-ctc-zh_en-int8-2026-02-25", "model*.onnx"),
        tokens=g("sherpa-onnx-fire-red-asr2-ctc-zh_en-int8-2026-02-25", "tokens.txt"),
        num_threads=THREADS)),
    "whisper-turbo": ("offline", lambda: sherpa_onnx.OfflineRecognizer.from_whisper(
        encoder=g("sherpa-onnx-whisper-turbo", "*encoder*.onnx"),
        decoder=g("sherpa-onnx-whisper-turbo", "*decoder*.onnx"),
        tokens=g("sherpa-onnx-whisper-turbo", "*tokens.txt"),
        language="", task="transcribe", num_threads=THREADS)),
    "x-asr-offline-int8": ("offline", lambda: sherpa_onnx.OfflineRecognizer.from_transducer(
        encoder=g("sherpa-onnx-x-asr-zipformer-transducer-zh-en-punct-int8-2026-06-03", "encoder*.onnx"),
        decoder=g("sherpa-onnx-x-asr-zipformer-transducer-zh-en-punct-int8-2026-06-03", "decoder*.onnx"),
        joiner=g("sherpa-onnx-x-asr-zipformer-transducer-zh-en-punct-int8-2026-06-03", "joiner*.onnx"),
        tokens=g("sherpa-onnx-x-asr-zipformer-transducer-zh-en-punct-int8-2026-06-03", "tokens.txt"),
        num_threads=THREADS)),
    "x-asr-480ms-streaming-int8": ("online", lambda: sherpa_onnx.OnlineRecognizer.from_transducer(
        encoder=g("sherpa-onnx-x-asr-480ms-streaming-zipformer-transducer-zh-en-punct-int8-2026-06-05", "encoder*.onnx"),
        decoder=g("sherpa-onnx-x-asr-480ms-streaming-zipformer-transducer-zh-en-punct-int8-2026-06-05", "decoder*.onnx"),
        joiner=g("sherpa-onnx-x-asr-480ms-streaming-zipformer-transducer-zh-en-punct-int8-2026-06-05", "joiner*.onnx"),
        tokens=g("sherpa-onnx-x-asr-480ms-streaming-zipformer-transducer-zh-en-punct-int8-2026-06-05", "tokens.txt"),
        num_threads=THREADS)),
    "zipformer-multilingual-streaming": ("online", lambda: sherpa_onnx.OnlineRecognizer.from_transducer(
        encoder=g("sherpa-onnx-streaming-zipformer-ar_en_id_ja_ru_th_vi_zh-2025-02-10", "encoder*.onnx"),
        decoder=g("sherpa-onnx-streaming-zipformer-ar_en_id_ja_ru_th_vi_zh-2025-02-10", "decoder*.onnx"),
        joiner=g("sherpa-onnx-streaming-zipformer-ar_en_id_ja_ru_th_vi_zh-2025-02-10", "joiner*.onnx"),
        tokens=g("sherpa-onnx-streaming-zipformer-ar_en_id_ja_ru_th_vi_zh-2025-02-10", "tokens.txt"),
        num_threads=THREADS)),
}

def load_clips():
    clips = []
    for wav in sorted((HERE / "clips").glob("*.wav")):
        txt = wav.with_suffix(".txt")
        if not txt.exists():
            continue
        audio, sr = sf.read(wav, dtype="float32")
        if audio.ndim > 1:
            audio = audio[:, 0]
        assert sr == 16000, f"{wav}: {sr} Hz, expected 16000"
        cat = wav.with_suffix(".cat").read_text().strip() if wav.with_suffix(".cat").exists() else "mix"
        clips.append((wav.stem, cat, audio, txt.read_text().strip()))
    return clips

def transcribe(kind, rec, audio):
    if kind == "offline":
        s = rec.create_stream()
        s.accept_waveform(16000, audio)
        rec.decode_stream(s)
        return s.result.text
    s = rec.create_stream()
    for i in range(0, len(audio), 1600):  # 100 ms chunks, as the mic loop will
        s.accept_waveform(16000, audio[i:i + 1600])
        while rec.is_ready(s):
            rec.decode_stream(s)
    s.input_finished()
    while rec.is_ready(s):
        rec.decode_stream(s)
    return rec.get_result(s)

def main():
    names = sys.argv[1:] or list(MODELS)
    clips = load_clips()
    if not clips:
        sys.exit("no clips with transcripts in clips/")
    results = json.loads((HERE / "results.json").read_text()) if (HERE / "results.json").exists() else {}  # merge with earlier runs
    for name in names:
        kind, build = MODELS[name]
        try:
            t0 = time.time(); rec = build(); load_s = time.time() - t0
        except FileNotFoundError as e:
            print(f"skip {name}: missing {e}"); continue
        transcribe(kind, rec, clips[0][2])  # warm-up
        rows, wall, audio_s = [], 0.0, 0.0
        for cid, cat, audio, ref in clips:
            t0 = time.time(); hyp = transcribe(kind, rec, audio); dt = time.time() - t0
            wall += dt; audio_s += len(audio) / 16000
            rows.append({"clip": cid, "cat": cat, "ref": ref, "hyp": hyp, "mer": mer(ref, hyp),
                         "trad": trad_rate(hyp), "sec": dt})
            print(f"{name:34s} {cid} [{cat}] mer={rows[-1]['mer']:.3f} {hyp}")
        results[name] = {"kind": kind, "load_s": load_s, "rtf": wall / audio_s, "rows": rows}
        del rec
    write_report(results)

def avg(xs):
    xs = [x for x in xs if x is not None]
    return sum(xs) / len(xs) if xs else None

def write_report(results):
    (HERE / "results.json").write_text(json.dumps(results, ensure_ascii=False, indent=1))
    cats = ["zh", "en", "mix", "fix"]
    lines = ["# ASR eval", "", "MER = mixed error rate (char-level zh, word-level en). trad = share of CJK output already traditional (before OpenCC). ms/10s = inference time per 10 s of audio.", "",
             "| model | kind | MER all | " + " | ".join(f"MER {c}" for c in cats) + " | trad | ms/10s |",
             "|---|---|---|" + "---|" * len(cats) + "---|---|"]
    for name, r in sorted(results.items(), key=lambda kv: avg([x["mer"] for x in kv[1]["rows"]])):
        rows = r["rows"]
        per = [avg([x["mer"] for x in rows if x["cat"] == c]) for c in cats]
        fmt = lambda v: "-" if v is None else f"{v:.3f}"
        lines.append(f"| {name} | {r['kind']} | {fmt(avg([x['mer'] for x in rows]))} | " + " | ".join(fmt(v) for v in per)
                     + f" | {fmt(avg([x['trad'] for x in rows]))} | {r['rtf'] * 10000:.0f} |")
    (HERE / "results.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))

if __name__ == "__main__":
    main()
