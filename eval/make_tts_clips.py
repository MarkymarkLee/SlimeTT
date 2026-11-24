"""Synthesize sanity clips from prompts.txt with macOS `say` (pipeline smoke test; not a substitute for real recordings)."""
import pathlib, subprocess
HERE = pathlib.Path(__file__).parent
OUT = HERE / "clips"
OUT.mkdir(exist_ok=True)
i = 900  # keep away from real clips (000..)
for line in (HERE / "prompts.txt").read_text().splitlines():
    if not line or line.startswith("#"):
        continue
    cat, text = line.split("|", 1)
    voice = "Eddy (English (US))" if cat == "en" else "Eddy (Chinese (Taiwan))"
    wav = OUT / f"{i:03d}.wav"
    subprocess.run(["say", "-v", voice, "-o", str(wav), "--file-format=WAVE", "--data-format=LEI16@16000", text], check=True)
    wav.with_suffix(".txt").write_text(text + "\n")
    wav.with_suffix(".cat").write_text(cat + "\n")
    i += 1
print(f"wrote {i-900} clips to {OUT}")
