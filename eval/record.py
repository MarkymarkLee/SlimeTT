"""Record eval clips. Prompted mode reads prompts.txt; free mode records spontaneous clips.

  .venv/bin/python record.py              # prompted: shows each line, Enter=start, Enter=stop
  .venv/bin/python record.py --free 10    # 10 spontaneous clips; type the transcript after each

Clips land in clips/NNN.wav (16 kHz mono) with clips/NNN.txt (gold) and clips/NNN.cat (category).
"""
import sys, queue, pathlib, sounddevice as sd, soundfile as sf, numpy as np

SR = 16000
CLIPS = pathlib.Path(__file__).parent / "clips"
CLIPS.mkdir(exist_ok=True)

def next_idx():
    return max([int(p.stem) for p in CLIPS.glob("*.wav") if p.stem.isdigit()] + [-1]) + 1

def record_one():
    q = queue.Queue()
    with sd.InputStream(samplerate=SR, channels=1, dtype="float32", callback=lambda d, *_: q.put(d.copy())):
        input("  [recording] press Enter to stop ")
    chunks = []
    while not q.empty():
        chunks.append(q.get())
    return np.concatenate(chunks)[:, 0] if chunks else np.zeros(0, np.float32)

def save(idx, audio, text, cat):
    sf.write(CLIPS / f"{idx:03d}.wav", audio, SR)
    (CLIPS / f"{idx:03d}.txt").write_text(text.strip() + "\n")
    (CLIPS / f"{idx:03d}.cat").write_text(cat + "\n")
    print(f"  saved {idx:03d} ({len(audio)/SR:.1f}s)")

def main():
    idx = next_idx()
    if len(sys.argv) > 1 and sys.argv[1] == "--free":
        for _ in range(int(sys.argv[2])):
            input(f"\n#{idx:03d} free clip: press Enter to start, speak, Enter to stop ")
            audio = record_one()
            cat = input("  category (zh/en/mix/fix): ").strip() or "mix"
            text = input("  exact transcript (you can edit clips/NNN.txt later): ")
            save(idx, audio, text, cat); idx += 1
        return
    done = {p.read_text().strip() for p in CLIPS.glob("*.txt")}
    for line in (pathlib.Path(__file__).parent / "prompts.txt").read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        cat, text = line.split("|", 1)
        if text in done:
            continue
        print(f"\n#{idx:03d} [{cat}] read this:\n  {text}")
        if input("  Enter to record, s to skip: ").strip() == "s":
            continue
        save(idx, record_one(), text, cat); idx += 1

if __name__ == "__main__":
    main()
