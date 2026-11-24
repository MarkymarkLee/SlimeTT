"""Shared scoring: mixed zh/en error rate, traditional-character rate."""
import re, jiwer, opencc

_s2t = opencc.OpenCC("s2t")
_s2twp = opencc.OpenCC("s2twp")
_TOK = re.compile(r"[一-鿿]|[A-Za-z0-9']+")

def tokens(s):
    """CJK chars are one token each; latin words are lowercased tokens; punctuation dropped."""
    return [t.lower() for t in _TOK.findall(s)]

def mer(ref, hyp):
    """Mixed error rate after OpenCC s2twp on both sides: WER over the token stream (char-level CJK, word-level latin)."""
    r, h = tokens(_s2twp.convert(ref)), tokens(_s2twp.convert(hyp))  # app always applies s2twp
    if not r:
        return 0.0 if not h else 1.0
    return jiwer.wer(" ".join(r), " ".join(h)) if h else 1.0

def trad_rate(s):
    """Share of CJK chars that are already traditional (unchanged by OpenCC s2t). None if no CJK."""
    cjk = [c for c in s if "一" <= c <= "鿿"]
    if not cjk:
        return None
    return sum(1 for c in cjk if _s2t.convert(c) == c) / len(cjk)

if __name__ == "__main__":
    assert tokens("我今天要跟 team 開 Meeting。") == ["我", "今", "天", "要", "跟", "team", "開", "meeting"]
    assert mer("hello world", "hello world") == 0.0
    assert abs(mer("我今天 team", "我明天 team") - 0.25) < 1e-9
    assert mer("這個軟體", "这个软件") == 0.0
    assert trad_rate("软") == 0.0 and trad_rate("軟體") == 1.0 and trad_rate("abc") is None
    print("metrics ok")
