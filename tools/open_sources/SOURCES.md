# Open data sources used by `tools/build_open_table.py`

Only the license files are committed. The data files are (re)downloaded with
`python tools/build_open_table.py --download`; their SHA-256 at build time is recorded in
`data/open/build_stats.json` (`sources_sha256`).

| Local path | Upstream URL | License (verified from the downloaded license text) | What we use |
|---|---|---|---|
| `unihan/Unihan.zip` → `Unihan_*.txt` | https://www.unicode.org/Public/UCD/latest/ucd/Unihan.zip | Unicode License v3 — [`unihan/LICENSE-Unicode.txt`](unihan/LICENSE-Unicode.txt) | kMandarin, kHanyuPinlu, kTGHZ2013, kXHC1983, kHanyuPinyin (readings); kRSUnicode (radical + residual strokes); kGB0, kTGH (character sets); kCangjie (optional, `--shape2 cangjie` only) |
| Python `unicodedata` (Kangxi radical names U+2F00–U+2FD5) | bundled with CPython (UCD) | Unicode License v3 | English radical names used as mnemonics for the radical → key table |
| `jieba/dict.txt` | https://raw.githubusercontent.com/fxsjy/jieba/master/jieba/dict.txt | MIT — [`jieba/LICENSE-jieba.txt`](jieba/LICENSE-jieba.txt) (Copyright (c) 2013 Sun Junyi) | word list + frequency (phrase selection and ranking only; POS tags ignored) |

Deliberately **not** used (license not compatible with a no-risk public release or provenance
unclear): anything from 小鹤音形 / flypy / openfly / rime-flypy, CC-CEDICT (CC BY-SA),
mozillazg/phrase-pinyin-data and the merged `pinyin.txt` of mozillazg/pinyin-data (they merge
CC BY-SA and web-scraped dictionary sources), rime-ice (GPL-3.0), rime-essay (LGPL), Sogou/QQ
cell dictionaries.
