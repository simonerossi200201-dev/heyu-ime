#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
HEYU (鹤羽) open default table builder
======================================

Builds the *distributable* default code table for HEYU from openly licensed sources only:

* Unicode Unihan database (Unicode License v3)
    - readings: kMandarin, kHanyuPinlu, kTGHZ2013, kXHC1983, kHanyuPinyin
    - shape:    kRSUnicode (Kangxi radical + residual strokes), kCangjie (last component letter)
    - sets:     kGB0 (GB 2312), kTGH (通用规范汉字表 8105)
* Python's bundled Unicode character names for the 214 Kangxi radicals (Unicode License v3)
* jieba dict.txt (MIT) - word list + corpus frequencies used only for phrase selection and ranking

Nothing from 小鹤 / flypy / openfly (or any EULA / NC / SA licensed table) is read here.

Outputs (same file formats as the private tables, so the engine loads them unchanged):
    data/open/heyu_open_sys.tsv          code<TAB>word<TAB>rank<TAB>layer
    data/open/heyu_open_reverse.tsv      char<TAB>code,code,...
    data/open/heyu_open_shape_hints.tsv  sp2<TAB>char:shape|char:shape|...
    data/open/build_stats.json           coverage / collision statistics

Scheme (see docs/OPEN_SCHEME.md):
    single character  = 2 sound keys (double-pinyin key layout) + 2 shape keys (our own design)
        shape key 1   = Kangxi radical -> key, via the RADICAL_KEY table below (designed for HEYU,
                        based on English radical names, NOT on any other IME's root/字根 table)
        shape key 2   = residual stroke count (strokes outside the radical, Unihan kRSUnicode)
                        -> keyboard row position: 0=z, 1..10=q..p, 11..19=a..l, 20..25+=x..m
                        (optional --shape2 cangjie: last letter of the Unihan kCangjie code)
    2-char phrase     = AaBb  (full double-pinyin of both characters)
    3-char phrase     = ABCc  (initial key of char 1, 2, full double-pinyin of char 3)
    4+-char phrase    = ABCZ  (initial key of chars 1, 2, 3 and the last character)

Usage:
    python tools/build_open_table.py [--download] [--shape2 cangjie|strokes] [--min-freq N]
"""

import argparse
import collections
import hashlib
import json
import os
import sys
import unicodedata
import urllib.request
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "tools", "open_sources")
OUT = os.path.join(ROOT, "data", "open")

SOURCES = {
    "unihan/Unihan.zip": "https://www.unicode.org/Public/UCD/latest/ucd/Unihan.zip",
    "unihan/LICENSE-Unicode.txt": "https://www.unicode.org/license.txt",
    "jieba/dict.txt": "https://raw.githubusercontent.com/fxsjy/jieba/master/jieba/dict.txt",
    "jieba/LICENSE-jieba.txt": "https://raw.githubusercontent.com/fxsjy/jieba/master/LICENSE",
}

# ----------------------------------------------------------------------------------------------
# Sound part: the double-pinyin key layout (functional key mapping; widely shipped, e.g. as an
# option of Microsoft Pinyin). Only the *layout* is used; readings come from Unihan.
# ----------------------------------------------------------------------------------------------
SP_INITIALS = {"zh": "v", "ch": "i", "sh": "u"}
SP_FINALS = {
    "iu": "q", "ei": "w", "e": "e", "uan": "r", "ue": "t", "ve": "t", "un": "y", "u": "u",
    "i": "i", "uo": "o", "o": "o", "ie": "p", "a": "a", "ong": "s", "iong": "s", "ai": "d",
    "en": "f", "eng": "g", "ang": "h", "an": "j", "ing": "k", "uai": "k", "iang": "l",
    "uang": "l", "ou": "z", "ia": "x", "ua": "x", "ao": "c", "ui": "v", "v": "v", "in": "b",
    "iao": "n", "ian": "m",
}
SP_ZERO = {"a": "aa", "o": "oo", "e": "ee", "ai": "ai", "ei": "ei", "ao": "ao", "ou": "ou",
           "an": "an", "en": "en", "er": "er", "ang": "ah", "eng": "eg"}
INITIALS = ["zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q",
            "x", "r", "z", "c", "s", "y", "w"]

# ----------------------------------------------------------------------------------------------
# Shape key 1: Kangxi radical -> key. HEYU's own design: the key is the first letter of an
# English keyword for the radical. The default keyword is the radical's Unicode name
# ("KANGXI RADICAL WATER" -> water -> w); the overrides below pick another English keyword
# (or a mnemonic) to spread load over the keyboard. Documented in docs/OPEN_SCHEME.md.
# ----------------------------------------------------------------------------------------------
RADICAL_KEYWORD_OVERRIDES = {
    1: "a (= one)",            # 一
    9: "person",               # 人 亻
    46: "k2 (a mountain)",     # 山
    61: "xoxo (heart)",        # 心 忄
    74: "luna",                # 月 (moon)
    130: "luna (月 form)",     # 肉 月 - same glyph as moon in simplified script
    85: "water",               # 氵
    112: "rock",               # 石
    104: "ill",                # 疒
    113: "altar",              # 示 礻
    120: "yarn",               # 糸 纟
    149: "quote (speech)",     # 言 讠
    162: "via (walk)",         # 辵 辶
    144: "via (walk enclosure)",  # 行
    167: "ore (gold)",         # 金 钅
    170: "hill (mound 阝 left)",  # 阜 阝
    38: "female",              # 女
    72: "day (sun)",           # 日
    157: "zapato (foot)",      # 足 ⻊  (Spanish 'shoe')
}


def radical_key(n: int) -> str:
    kw = RADICAL_KEYWORD_OVERRIDES.get(n)
    if kw is None:
        kw = unicodedata.name(chr(0x2F00 + n - 1)).replace("KANGXI RADICAL ", "").lower()
    return kw[0]


def radical_keyword(n: int) -> str:
    return RADICAL_KEYWORD_OVERRIDES.get(n) or unicodedata.name(chr(0x2F00 + n - 1)).replace("KANGXI RADICAL ", "").lower()


# Shape key 2 (alternative "strokes" mode): residual stroke count -> keyboard row position.
STROKE_KEYS = {0: "z"}
STROKE_KEYS.update({i + 1: k for i, k in enumerate("qwertyuiop")})      # 1..10
STROKE_KEYS.update({i + 11: k for i, k in enumerate("asdfghjkl")})      # 11..19
STROKE_KEYS.update({i + 20: k for i, k in enumerate("xcvbnm")})         # 20..25 (25+ = m)


def stroke_key(n: int) -> str:
    return STROKE_KEYS.get(min(max(n, 0), 25), "m")


# Fast punctuation after ';' - HEYU's own mnemonic set.
FAST_SYMBOLS = [
    (";", "；"), (";d", "、"), (";h", "——"), (";l", "……"), (";m", "："), (";w", "？"),
    (";g", "！"), (";y", "“"), (";u", "”"), (";j", "「"), (";k", "」"), (";q", "《"),
    (";p", "》"), (";o", "（"), (";i", "）"), (";a", "@"), (";x", "×"), (";t", "~"),
    (";c", "·"), (";b", "【"), (";n", "】"), (";e", "="), (";r", "→"), (";s", "￥"),
    (";f", "%"), (";z", "_"), (";v", "|"),
]


# ----------------------------------------------------------------------------------------------
def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def download():
    for rel, url in SOURCES.items():
        dst = os.path.join(SRC, rel)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        print(f"[download] {url}")
        urllib.request.urlretrieve(url, dst)
    with zipfile.ZipFile(os.path.join(SRC, "unihan", "Unihan.zip")) as z:
        z.extractall(os.path.join(SRC, "unihan"))


def strip_tone(py: str):
    """'xíng' -> 'xing', 'lǜ' -> 'lv'. Returns None for syllables outside the scheme (m, ng, hm...)."""
    s = unicodedata.normalize("NFD", py.strip().lower())
    s = s.replace("u\u0308", "v").replace("ü", "v")
    s = "".join(c for c in s if not unicodedata.combining(c))
    s = s.replace("ê", "e")
    if not s.isalpha() or not s.isascii():
        return None
    return s


def to_sp(py: str):
    if py is None:
        return None
    if py in SP_ZERO:
        return SP_ZERO[py]
    for ini in INITIALS:
        if py.startswith(ini) and len(py) > len(ini):
            fin = py[len(ini):]
            if ini in ("j", "q", "x", "y") and fin.startswith("v"):
                fin = "u" + fin[1:]  # ju/qu/xu/yu are written with u
            if ini in ("j", "q", "x", "y") and fin == "ue":
                fin = "ue"
            key = SP_FINALS.get(fin)
            if key is None:
                return None
            return SP_INITIALS.get(ini, ini) + key
    return None


def load_unihan():
    U = os.path.join(SRC, "unihan")
    want = {"kMandarin", "kHanyuPinlu", "kTGHZ2013", "kXHC1983", "kHanyuPinyin", "kRSUnicode",
            "kTotalStrokes", "kCangjie", "kGB0", "kTGH"}
    data = collections.defaultdict(dict)
    for fn in ("Unihan_Readings.txt", "Unihan_IRGSources.txt", "Unihan_DictionaryLikeData.txt",
               "Unihan_OtherMappings.txt"):
        with open(os.path.join(U, fn), encoding="utf-8") as f:
            for line in f:
                if line.startswith("#") or not line.strip():
                    continue
                cp, field, val = line.rstrip("\n").split("\t", 2)
                if field in want:
                    data[chr(int(cp[2:], 16))][field] = val
    return data


def readings_of(info):
    """Ordered list of (pinyin_without_tone, weight) for a character; first = primary."""
    order = []
    weights = {}

    def add(py, w):
        p = strip_tone(py)
        if p is None or to_sp(p) is None:
            return
        if p not in weights:
            order.append(p)
            weights[p] = w
        else:
            weights[p] = max(weights[p], w)

    if "kHanyuPinlu" in info:
        items = []
        for tok in info["kHanyuPinlu"].split():
            py, cnt = tok.rstrip(")").split("(")
            items.append((int(cnt), py))
        items.sort(key=lambda x: -x[0])
        top = items[0][0] or 1
        for cnt, py in items:
            add(py, cnt / top)
    if "kMandarin" in info:
        for py in info["kMandarin"].split():
            add(py, 0.5)
    if "kTGHZ2013" in info:
        for tok in info["kTGHZ2013"].split():
            for py in tok.split(":", 1)[1].split(","):
                add(py, 0.2)
    if not order and "kXHC1983" in info:
        for tok in info["kXHC1983"].split():
            for py in tok.split(":", 1)[1].split(","):
                add(py, 0.2)
    if not order and "kHanyuPinyin" in info:
        first = info["kHanyuPinyin"].split()[0].split(":", 1)[1].split(",")[0]
        add(first, 0.1)
    return [(p, weights[p]) for p in order]


def radical_of(info):
    v = info.get("kRSUnicode", "").split()
    if not v:
        return None, None
    rad, _, rest = v[0].partition(".")
    rad = int(rad.rstrip("'\"!"))
    try:
        residual = int(rest)
    except ValueError:
        residual = 0
    return rad, residual


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--download", action="store_true", help="(re)download the open sources first")
    ap.add_argument("--shape2", choices=("strokes", "cangjie"), default="strokes",
                    help="shape key 2: residual stroke count (default) or last kCangjie letter")
    ap.add_argument("--min-freq", type=int, default=20, help="minimum jieba frequency for phrases")
    ap.add_argument("--max-phrase-len", type=int, default=8)
    ap.add_argument("--out", default=OUT)
    args = ap.parse_args()

    if args.download or not os.path.exists(os.path.join(SRC, "unihan", "Unihan_Readings.txt")):
        download()

    uni = load_unihan()
    charset = {c for c, i in uni.items() if "kGB0" in i or "kTGH" in i}
    gb2312 = {c for c, i in uni.items() if "kGB0" in i}

    # jieba words & frequencies
    words = {}
    with open(os.path.join(SRC, "jieba", "dict.txt"), encoding="utf-8") as f:
        for line in f:
            p = line.split()
            if len(p) >= 2 and p[1].isdigit():
                words[p[0]] = max(words.get(p[0], 0), int(p[1]))
    occ = collections.Counter()   # character occurrence frequency
    for w, fr in words.items():
        for ch in w:
            occ[ch] += fr
    single = {c: words.get(c, 0) for c in charset}

    readings = {}
    for c in charset:
        r = readings_of(uni[c])
        if r:
            readings[c] = r

    def shape(c):
        info = uni[c]
        rad, residual = radical_of(info)
        if rad is None:
            return None
        k1 = radical_key(rad)
        if args.shape2 == "cangjie" and info.get("kCangjie"):
            k2 = info["kCangjie"][-1].lower()
        else:
            k2 = stroke_key(residual)
        return k1 + k2

    entries = collections.defaultdict(list)   # code -> [(score, word, layer)]
    reverse = collections.defaultdict(list)
    char_primary_code = {}

    def add(code, word, score, layer):
        for s, w, l in entries[code]:
            if w == word:
                return
        entries[code].append((score, word, layer))

    # ---- single characters: full codes
    for c, rlist in readings.items():
        shp = shape(c)
        if not shp:
            continue
        base = occ.get(c, 0) + single.get(c, 0) + 1
        for i, (py, w) in enumerate(rlist[:4]):
            sp = to_sp(py)
            code = sp + shp
            layer = "primary" if (i == 0 and (c in gb2312 or base > 50)) else "secondary"
            add(code, c, base * max(w, 0.05) * (1.0 if i == 0 else 0.6), layer)
            if code not in reverse[c]:
                reverse[c].append(code)
            if i == 0:
                char_primary_code[c] = code

    # ---- phrases
    def char_options(c, allow_alt):
        rl = readings[c]
        opts = [to_sp(rl[0][0])]
        if allow_alt:
            for py, w in rl[1:3]:
                if w >= 0.1:
                    sp = to_sp(py)
                    if sp not in opts:
                        opts.append(sp)
        return opts

    def phrase_codes(w):
        n = len(w)
        allow_alt = True
        opts = [char_options(c, allow_alt) for c in w]
        if n == 2:
            pick = [opts[0], opts[1]]
            form = lambda a: a[0] + a[1]
        elif n == 3:
            pick = [sorted({o[0] for o in opts[0]}), sorted({o[0] for o in opts[1]}), opts[2]]
            form = lambda a: a[0] + a[1] + a[2]
        else:
            pick = [sorted({o[0] for o in opts[0]}), sorted({o[0] for o in opts[1]}),
                    sorted({o[0] for o in opts[2]}), sorted({o[0] for o in opts[-1]})]
            form = lambda a: "".join(a)
        primary = form([p[0] for p in pick])
        codes = [primary]

        def rec(i, acc):
            if len(codes) > 4:
                return
            if i == len(pick):
                code = form(acc)
                if code not in codes:
                    codes.append(code)
                return
            for o in pick[i]:
                rec(i + 1, acc + [o])
        rec(0, [])
        return codes[:4]

    n_phr = 0
    phrase_count_by_len = collections.Counter()
    for w, fr in words.items():
        n = len(w)
        if n < 2 or n > args.max_phrase_len or fr < args.min_freq:
            continue
        if not all(c in readings for c in w):
            continue
        codes = phrase_codes(w)
        for j, code in enumerate(codes):
            add(code, w, fr * (1.0 if j == 0 else 0.5), "primary" if j == 0 else "secondary")
        n_phr += 1
        phrase_count_by_len[min(n, 5)] += 1

    # ---- short codes (algorithmic, by standalone single-character frequency)
    by_initial = collections.defaultdict(list)
    by_syllable = collections.defaultdict(list)
    for c, code in char_primary_code.items():
        if c not in gb2312:
            continue
        by_initial[code[0]].append((single.get(c, 0) + occ.get(c, 0) // 50, c))
        by_syllable[code[:2]].append((single.get(c, 0) + occ.get(c, 0) // 50, c))
    short = []   # (code, word, rank, layer)
    for k, lst in sorted(by_initial.items()):
        lst.sort(reverse=True)
        if lst:
            short.append((k, lst[0][1], 0, "primary"))
        if len(lst) > 1:
            short.append((k, lst[1][1], 1, "short2"))
    for sp2, lst in sorted(by_syllable.items()):
        lst.sort(reverse=True)
        for i, (_, c) in enumerate(lst[:2]):
            short.append((sp2, c, i, "primary"))
    # 3-key prefixes: up to 3 most frequent characters, then the most frequent 2-char phrase
    pre3_chars = collections.defaultdict(list)
    for c, code in char_primary_code.items():
        if c in gb2312:
            pre3_chars[code[:3]].append((occ.get(c, 0) + single.get(c, 0), c))
    pre3_phr = {}
    for code, lst in entries.items():
        if len(code) == 4 and code.isalpha():
            for s, w, l in lst:
                if len(w) == 2 and l == "primary":
                    if code[:3] not in pre3_phr or pre3_phr[code[:3]][0] < s:
                        pre3_phr[code[:3]] = (s, w)
    for p3 in sorted(set(pre3_chars) | set(pre3_phr)):
        lst = sorted(pre3_chars.get(p3, []), reverse=True)[:3]
        rank = 0
        for _, c in lst:
            short.append((p3, c, rank, "primary"))
            rank += 1
        if p3 in pre3_phr and pre3_phr[p3][0] >= 1000:
            short.append((p3, pre3_phr[p3][1], rank, "primary"))

    # ---- write sys table
    os.makedirs(args.out, exist_ok=True)
    rows = []
    for code, w in FAST_SYMBOLS:
        rows.append((code, w, 0, "fast_symbol"))
    short_codes = collections.defaultdict(list)
    for code, w, r, l in short:
        short_codes[code].append((r, w, l))
    for code in sorted(short_codes):
        seen = set()
        for r, w, l in sorted(short_codes[code]):
            if w in seen:
                continue
            seen.add(w)
            rows.append((code, w, r, l))
            if len(w) == 1 and code not in reverse[w]:
                reverse[w].append(code)
    for code in sorted(entries):
        lst = sorted(entries[code], key=lambda x: -x[0])
        for r, (s, w, l) in enumerate(lst):
            if code in short_codes and any(w == x[1] for x in short_codes[code]):
                continue
            rows.append((code, w, r, l))

    sys_path = os.path.join(args.out, "heyu_open_sys.tsv")
    with open(sys_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("# HEYU open default table - generated by tools/build_open_table.py\n")
        f.write("# Sources: Unicode Unihan (Unicode License v3), jieba dict.txt (MIT). See docs/LICENSING.md\n")
        f.write("# Format: code<TAB>word<TAB>rank<TAB>layer\n")
        for code, w, r, l in rows:
            f.write(f"{code}\t{w}\t{r}\t{l}\n")

    rev_path = os.path.join(args.out, "heyu_open_reverse.tsv")
    with open(rev_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("# HEYU open reverse lookup (char -> codes) - generated by tools/build_open_table.py\n")
        for c in sorted(reverse, key=lambda x: (-(occ.get(x, 0)), x)):
            full = [x for x in reverse[c] if len(x) == 4]
            rest = [x for x in reverse[c] if len(x) != 4]
            f.write(f"{c}\t{','.join(full + rest)}\n")

    hints = collections.defaultdict(list)
    for c, code in sorted(char_primary_code.items(), key=lambda kv: -(occ.get(kv[0], 0) + single.get(kv[0], 0))):
        if c in gb2312 and len(hints[code[:2]]) < 10:
            hints[code[:2]].append(f"{c}:{code[2:]}")
    hint_path = os.path.join(args.out, "heyu_open_shape_hints.tsv")
    with open(hint_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("# HEYU open double-pinyin -> shape suffix hints - generated by tools/build_open_table.py\n")
        f.write("# Format: sp2<TAB>char:shape_suffix|char:shape_suffix|...\n")
        for sp2 in sorted(hints):
            f.write(f"{sp2}\t{'|'.join(hints[sp2])}\n")

    # ---- statistics
    gb_cov = sum(1 for c in gb2312 if c in char_primary_code)
    four = {code: lst for code, lst in entries.items() if len(code) == 4 and code.isalpha()}
    multi = sum(1 for lst in four.values() if len(lst) > 1)
    char_codes = collections.defaultdict(list)
    for c, code in char_primary_code.items():
        if c in gb2312:
            char_codes[code].append(c)
    gb_unique = sum(1 for c in gb2312 if c in char_primary_code and len(char_codes[char_primary_code[c]]) == 1)
    first_choice = 0
    for c in gb2312:
        code = char_primary_code.get(c)
        if code:
            top = max(entries[code], key=lambda x: x[0])[1]
            if top == c:
                first_choice += 1
    samples = {}
    for c in "的一是不了人我在有他这中大来上国个到说们为子和你地出道也时年得就那要下以生会自着去之过家学对可她里后小么心多天而能好都然没日于起还发成事只作当想看文无开手十用主行方又如前所本见经头面公同三已老从动两长知民样现分将外但身些与高意进把法此实回二理美点月明其种声全工己话儿者向情部正名定女问力机给等几很业最间新什打便位因重被走电四第门相次东政海口使教西再平真听世气信北少关并内加化由却代军产入先山五太水万市眼体别处总才场师书比住员九笑性通目华报立马命张活难神数件安表原车白应路期叫死常提感金何更反合放做系计或司利受光王果亲界及今京务制解各任至清物台象记边共风战干接它许八特觉望直服毛林题建南度统色字请交爱让认算论百吃义科怎元社术结六功指思非流每青管夫连远资队跟带花快条院变联言权往展该领传近留红治决周保达办运武半候七必城父强步完革深区即求品士转量空甚众技轻程告江语英基派满式李息写呢识极令黄德收脸钱党倒未持取设始版双历越史商千片容研像找友孩站广改议形委早房音火际则首单据导影失拿网香似斯专石若兵弟谁校读志飞观争究包组造落视济喜离虽坏兴鹤羽":
        if c in char_primary_code:
            samples[c] = char_primary_code[c]
        if len(samples) >= 30:
            break
    for c in "鹤羽河和明好情请清晴林森":
        if c in char_primary_code:
            samples[c] = char_primary_code[c]
    for w in ("中国", "我们", "因为", "所以", "鹤羽", "输入法", "现代化", "实事求是", "中华人民共和国"):
        if all(c in readings for c in w):
            samples[w] = phrase_codes(w)[0]
    stats = {
        "shape2_mode": args.shape2,
        "min_phrase_freq": args.min_freq,
        "rows": len(rows),
        "single_chars_with_codes": len(char_primary_code),
        "gb2312_total": len(gb2312),
        "gb2312_covered": gb_cov,
        "gb2312_coverage_pct": round(gb_cov / len(gb2312) * 100, 2),
        "phrases": n_phr,
        "phrases_by_len(5=5+)": dict(sorted(phrase_count_by_len.items())),
        "four_key_codes": len(four),
        "four_key_codes_with_more_than_one_candidate": multi,
        "four_key_collision_rate_pct": round(multi / max(len(four), 1) * 100, 2),
        "gb2312_chars_with_unique_char_code_pct": round(gb_unique / max(gb_cov, 1) * 100, 2),
        "gb2312_chars_first_choice_at_full_code_pct": round(first_choice / max(gb_cov, 1) * 100, 2),
        "samples": samples,
        "sources_sha256": {
            rel: sha256(os.path.join(SRC, rel)) for rel in SOURCES if os.path.exists(os.path.join(SRC, rel))
        },
    }
    with open(os.path.join(args.out, "build_stats.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump(stats, f, ensure_ascii=False, indent=2)
    print(json.dumps({k: v for k, v in stats.items() if k not in ("samples", "sources_sha256")}, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
