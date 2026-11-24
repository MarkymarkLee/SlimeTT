"""Score oMLX chat models on the dictation-cleanup task.

  .venv/bin/python eval_llm.py [model ...]   # default: every model oMLX lists at :11434

Cases: fix/mix clips' prompts (raw spoken text -> intended text, from llm_cases.txt) plus any clips/NNN.intent.txt
you add next to a real recording (raw = clips/NNN.txt, intended = clips/NNN.intent.txt).
"""
import json, pathlib, sys, time, urllib.request
from metrics import mer, trad_rate

HERE = pathlib.Path(__file__).parent
URL = "http://127.0.0.1:11434"
SYSTEM = (HERE.parent / "assets/prompts/cleanup.txt").read_text()

def cases():
    out = []
    for line in (HERE / "llm_cases.txt").read_text().splitlines():
        if line and not line.startswith("#"):
            raw, intended = line.split("|", 1); intended = intended.replace("\\n", "\n")
            out.append((raw, intended))
    for intent in sorted((HERE / "clips").glob("*.intent.txt")):
        raw = intent.with_name(intent.name.replace(".intent", "")).read_text().strip()
        out.append((raw, intent.read_text().strip()))
    return out

def chat(model, raw):
    body = {"model": model, "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": raw}],
            "temperature": 0, "max_tokens": 400, "chat_template_kwargs": {"enable_thinking": False}}
    req = urllib.request.Request(f"{URL}/v1/chat/completions", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    t0 = time.time()
    with urllib.request.urlopen(req, timeout=300) as r:
        out = json.load(r)["choices"][0]["message"]["content"].strip()
    return out, time.time() - t0

def main():
    models = sys.argv[1:] or [m["id"] for m in json.load(urllib.request.urlopen(f"{URL}/v1/models"))["data"] if "asr" not in m["id"].lower() and "parakeet" not in m["id"].lower()]
    cs = cases()
    lines = ["# LLM cleanup eval", "", "MER vs intended text (after OpenCC on both). trad = traditional share before OpenCC. s = mean seconds per case.", "",
             "| model | MER | trad | s |", "|---|---|---|---|"]
    detail = {}
    for m in models:
        chat(m, cs[0][0])  # warm-up / load
        rows = []
        for raw, intended in cs:
            out, dt = chat(m, raw)
            rows.append({"raw": raw, "intended": intended, "out": out, "mer": mer(intended, out), "trad": trad_rate(out), "s": dt})
            print(f"{m:36s} mer={rows[-1]['mer']:.3f} {dt:.2f}s | {out}")
        detail[m] = rows
        a = lambda k: sum(r[k] for r in rows if r[k] is not None) / max(1, len([r for r in rows if r[k] is not None]))
        lines.append(f"| {m} | {a('mer'):.3f} | {a('trad'):.3f} | {a('s'):.2f} |")
    (HERE / "results_llm.json").write_text(json.dumps(detail, ensure_ascii=False, indent=1))
    (HERE / "results_llm.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))

if __name__ == "__main__":
    main()
