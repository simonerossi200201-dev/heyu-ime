//! v2 accuracy pipeline (HEYU v2):
//! * `sanitize_punct`: maps CJK "small form" / compatibility punctuation (U+FE50..U+FE6B, e.g. `﹖` `﹗` `﹐`)
//!   to standard full-width marks and drops overline/underline decorations (U+FE49..U+FE4F, e.g. `﹌`)
//!   and stray wave dashes, then collapses duplicated punctuation. Applied to Whisper output AND to the
//!   rolling context before it is fed back as `initial_prompt` (stops the self-reinforcing loop).
//! * `build_prompt_v2`: punctuated Simplified-Chinese example prompt + user hotwords + clean rolling tail.
//! * `postprocess_v2`: trusts Whisper's own punctuation (no length/pause-based rule punctuation).
//! * `llm_polish`: optional local Ollama pass through the separate `heyu-polish.exe` helper
//!   (loopback 127.0.0.1 only; heyu-voice.exe itself keeps ZERO network imports) restricted to punctuation /
//!   homophone / number fixes, with an edit-distance guard that rejects any paraphrase.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::Instant;

pub fn is_std_zh_punct(c: char) -> bool {
    matches!(c, '，' | '。' | '？' | '！' | '；' | '：' | '、' | '…' | '“' | '”' | '‘' | '’' | '（' | '）' | '《' | '》' | '—')
}

/// Map odd/compatibility punctuation to standard full-width marks; drop decorative marks.
pub fn sanitize_punct(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        let m = match c {
            '\u{FE50}' | '\u{FE10}' => Some('，'),
            '\u{FE51}' | '\u{FE11}' => Some('、'),
            '\u{FE52}' | '\u{FE12}' => Some('。'),
            '\u{FE54}' | '\u{FE14}' => Some('；'),
            '\u{FE55}' | '\u{FE13}' => Some('：'),
            '\u{FE56}' | '\u{FE16}' => Some('？'),
            '\u{FE57}' | '\u{FE15}' => Some('！'),
            '\u{FE59}' => Some('（'),
            '\u{FE5A}' => Some('）'),
            '\u{FE30}'..='\u{FE4F}' => None, // vertical forms / overlines / underscores (﹌ etc.)
            '\u{FE58}' | '\u{FE5B}'..='\u{FE6B}' => None,
            '\u{3030}' | '\u{301C}' | '～' => None, // wave dashes at pauses
            '\u{FFFD}' | '＜' | '＞' | '＊' | '＃' | '＠' | '＾' | '｀' | '｜' | '＿' => None,
            _ => Some(c),
        };
        if let Some(ch) = m {
            out.push(ch);
        }
    }
    collapse_punct(&out)
}

fn punct_rank(c: char) -> u8 {
    match c {
        '？' | '！' => 3,
        '。' => 2,
        '；' | '：' => 1,
        '，' | '、' => 0,
        _ => 0,
    }
}

/// `，，` -> `，`; `？？` -> `？`; `，。` -> `。`; `。，` -> `。`; leading punctuation removed.
pub fn collapse_punct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let is_p = matches!(c, '，' | '。' | '？' | '！' | '；' | '：' | '、');
        if is_p {
            match out.chars().last() {
                None => continue,
                Some(l) if matches!(l, '，' | '。' | '？' | '！' | '；' | '：' | '、') => {
                    if punct_rank(c) > punct_rank(l) {
                        out.pop();
                        out.push(c);
                    }
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

pub const ZH_PROMPT_V2: &str = "以下是一段简体中文口述，使用规范的标点符号。比如：现在这个语音识别工具在使用的时候准确率很高，不会识别错字，发出的文字可以直接使用，大约有30%到40%的提升。";

pub fn build_prompt_v2(hotwords: &[String], tail: &str) -> String {
    let mut p = String::from(ZH_PROMPT_V2);
    let hw: Vec<&str> = hotwords.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).take(40).collect();
    if !hw.is_empty() {
        p.push_str("常用词：");
        p.push_str(&hw.join("、"));
        p.push('。');
    }
    // The rolling tail is the channel through which one odd mark (`﹖`, `：`, `Ｐ` ...) poisons every
    // following chunk, so it is rebuilt with only `，。？！` before being shown to Whisper.
    let mode = std::env::var("HEYU_TAIL_MODE").unwrap_or_default();
    let t = if mode == "off" { String::new() } else if mode == "raw" { sanitize_punct(tail) } else { normalize_tail(tail) };
    let chars: Vec<char> = t.trim().chars().collect();
    if !chars.is_empty() {
        let keep = chars.len().min(60);
        p.extend(chars[chars.len() - keep..].iter());
    }
    p
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF)
}

fn starts_with_continuation_particle(s: &str) -> bool {
    let t = s.trim_start_matches(|c: char| is_std_zh_punct(c) || c.is_whitespace());
    let Some(first) = t.chars().next() else {
        return true;
    };
    first.is_ascii_digit()
        || matches!(
            first,
            '%' | '的' | '地' | '得' | '了' | '着' | '过' | '后' | '前' | '时' | '候'
                | '里' | '中' | '内' | '外' | '上' | '下' | '到' | '至' | '和' | '与'
                | '及' | '或' | '吗' | '呢' | '吧' | '啊' | '呀'
        )
}

/// v2 post-processing: normalize Whisper punctuation, avoid mid-phrase segment splitting,
/// and apply deterministic homophone + spoken disfluency repairs.
pub fn postprocess_v2(raw: &str, prompt: Option<&str>, smart: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    for line in raw.lines() {
        let l = crate::asr::strip_inline_watermark_hallucinations(line);
        let l = l.trim();
        if l.is_empty() || crate::asr::is_breath_hallucination(l) {
            continue;
        }
        let bracketed = (l.starts_with('[') && l.ends_with(']')) || (l.starts_with('(') && l.ends_with(')')) || (l.starts_with('（') && l.ends_with('）'));
        if bracketed {
            continue;
        }
        if let Some(p) = prompt {
            if l.chars().count() > 6 && p.contains(l) {
                continue;
            }
        }
        parts.push(l.to_string());
    }
    if parts.is_empty() {
        return String::new();
    }
    let mut glued = String::new();
    for p in &parts {
        let last = glued.chars().last();
        let first = p.chars().next();
        let prev_cjk_len = glued.chars().rev().take_while(|c| !is_std_zh_punct(*c)).filter(|c| is_cjk(*c)).count();
        let next_cjk_len = p.chars().filter(|c| is_cjk(*c)).count();
        let prev_incomplete = crate::corrector::is_syntactically_incomplete_zh_end(&glued)
            || glued.ends_with("变成")
            || glued.ends_with("变成了")
            || glued.ends_with("高达")
            || glued.ends_with("语音识别")
            || glued.ends_with("识别")
            || glued.ends_with("教会");
        let next_cont = starts_with_continuation_particle(p);
        let need_comma = match (last, first) {
            (None, _) => false,
            (Some(l), _) if is_std_zh_punct(l) || matches!(l, ',' | '.' | '?' | '!') => false,
            (Some(l), _) if l.is_ascii_digit() || l == '%' => false,
            (_, Some(f)) if is_std_zh_punct(f) || f == '%' => false,
            _ if prev_incomplete || next_cont || prev_cjk_len < 5 || next_cjk_len < 4 => false,
            _ => true,
        };
        if need_comma { glued.push('，'); }
        glued.push_str(p);
    }
    let joined = crate::corrector::merge_dictated_english_acronyms(&crate::corrector::to_simplified_zh(&glued));
    let ch: Vec<char> = joined.chars().collect();
    let mut s = String::with_capacity(joined.len());
    for (i, &c) in ch.iter().enumerate() {
        let prev = if i > 0 { Some(ch[i - 1]) } else { None };
        let next = ch.get(i + 1).copied();
        let near_cjk = prev.map(is_cjk).unwrap_or(false) || next.map(is_cjk).unwrap_or(false);
        let digits = prev.map(|p| p.is_ascii_digit()).unwrap_or(false) && next.map(|n| n.is_ascii_digit()).unwrap_or(false);
        if c.is_whitespace() {
            if near_cjk { continue; }
            if !s.ends_with(' ') { s.push(' '); }
            continue;
        }
        let latin_ctx = prev.map(|p| p.is_ascii_alphanumeric()).unwrap_or(false) && next.map(|n| n.is_ascii_alphanumeric() || n == ' ').unwrap_or(true) && !near_cjk;
        if c == '~' && !latin_ctx { continue; }
        let m = if !digits && !latin_ctx {
            match c { ',' => '，', '?' => '？', '!' => '！', ':' => '：', ';' => '；', '.' if near_cjk || prev.map(is_std_zh_punct).unwrap_or(false) => '。', _ => c }
        } else { c };
        s.push(m);
    }
    let s = fix_separator_artifacts(&sanitize_punct(s.trim()));
    let s = if smart {
        let s1 = crate::asr::apply_spoken_self_correction(&s);
        crate::corrector::correct_zh_homophones_and_collocations(&s1, None)
    } else {
        crate::corrector::correct_zh_homophones_and_collocations(&s, None)
    };
    let s = crate::corrector::collapse_asr_repetitions(&s);
    let s = sanitize_punct(&s);
    // Every committed phrase ends with a mark so the next phrase never glues onto it.
    match s.chars().last() {
        Some(c) if matches!(c, '，' | '。' | '？' | '！' | '；' | '：' | '、' | '…' | '”' | '）' | '.' | '?' | '!') => s,
        Some(_) => format!("{}，", s),
        None => s,
    }
}

// ------------------------------------------------------------------------------------------
// Optional local LLM polish (Ollama on loopback). Never paraphrases: guarded by edit distance.
// ------------------------------------------------------------------------------------------

pub struct PolishCfg {
    pub enabled: bool,
    pub model: String,
    pub timeout_ms: u64,
    pub hotwords: String,
}

fn bare(s: &str) -> Vec<char> {
    s.chars().filter(|c| !c.is_whitespace() && !is_std_zh_punct(*c) && !matches!(c, ',' | '.' | '?' | '!' | ';' | ':')).collect()
}

pub fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Accept the LLM edit when it fixes homophones, punctuation, and spoken hesitation fillers
/// without hallucinating chat responses or unrelated rewrites.
pub fn guard_accept(input: &str, output: &str) -> bool {
    let a = bare(input);
    let b = bare(output);
    if a.is_empty() || b.is_empty() { return false; }
    for bad_prefix in ["好的，", "好的：", "没问题", "修改后", "校对后", "校对结果", "以下是", "待校对"] {
        if output.starts_with(bad_prefix) {
            return false;
        }
    }
    let ratio = b.len() as f32 / a.len() as f32;
    if !(0.55..=1.25).contains(&ratio) { return false; }
    let d = levenshtein(&a, &b);
    d <= (a.len() * 45 / 100).max(6)
}


struct Helper {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

static HELPER: Mutex<Option<Helper>> = Mutex::new(None);

fn one_line(s: &str) -> String {
    s.chars().map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c }).collect()
}

/// Talk to `heyu-polish.exe` (same folder as heyu-voice.exe) over stdin/stdout, one line per request.
fn helper_call(line: &str) -> Option<String> {
    let mut g = HELPER.lock().ok()?;
    for attempt in 0..2 {
        if g.is_none() {
            let exe = std::env::current_exe().ok()?.parent()?.join("heyu-polish.exe");
            if !exe.is_file() {
                return None;
            }
            let mut cmd = Command::new(exe);
            cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            }
            let mut child = cmd.spawn().ok()?;
            let stdin = child.stdin.take()?;
            let stdout = BufReader::new(child.stdout.take()?);
            *g = Some(Helper { _child: child, stdin, stdout });
        }
        let h = g.as_mut().unwrap();
        let ok = h.stdin.write_all(line.as_bytes()).is_ok() && h.stdin.write_all(b"\n").is_ok() && h.stdin.flush().is_ok();
        let mut resp = String::new();
        if ok && h.stdout.read_line(&mut resp).map(|n| n > 0).unwrap_or(false) {
            return Some(resp.trim_end_matches(['\r', '\n']).to_string());
        }
        *g = None; // helper died: restart once
        let _ = attempt;
    }
    None
}

/// Load the polish model into VRAM (keep_alive) so the first real sentence is fast.
pub fn llm_warmup(cfg: &PolishCfg) -> bool {
    if !cfg.enabled {
        return false;
    }
    matches!(helper_call(&format!("W\t{}", one_line(&cfg.model))).as_deref(), Some(r) if r.starts_with("OK"))
}

/// Returns (text, accepted, ms). On any failure / timeout / rejected edit returns the input unchanged.
pub fn llm_polish(cfg: &PolishCfg, context: &str, text: &str) -> (String, bool, u128) {
    let t0 = Instant::now();
    if !cfg.enabled || text.chars().count() < 4 {
        return (text.to_string(), false, 0);
    }
    let ctx = sanitize_punct(context);
    let cc: Vec<char> = ctx.chars().collect();
    let ctx_tail: String = cc[cc.len().saturating_sub(80)..].iter().collect();
    let req = format!(
        "P\t{}\t{}\t{}\t{}\t{}",
        one_line(&cfg.model),
        cfg.timeout_ms,
        one_line(&cfg.hotwords),
        one_line(&ctx_tail),
        one_line(text)
    );
    let resp = helper_call(&req);
    let ms = t0.elapsed().as_millis();
    let Some(r) = resp else { return (text.to_string(), false, ms) };
    let Some(out) = r.strip_prefix("OK\t") else { return (text.to_string(), false, ms) };
    let out = out.trim().trim_start_matches("待校对：").trim().to_string();
    let out = sanitize_punct(&crate::corrector::to_simplified_zh(&out));
    if guard_accept(text, &out) {
        (out, true, ms)
    } else {
        (text.to_string(), false, ms)
    }
}

/// Reject Whisper loop hallucinations: drop repeated clauses and cap text length by audio duration
/// (Mandarin dictation rarely exceeds ~7 chars/s; we allow 9 chars/s + 6).
pub fn hallucination_guard(text: &str, audio_ms: usize) -> String {
    let mut clauses: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if matches!(c, '，' | '。' | '？' | '！' | '；') {
            clauses.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() { clauses.push(cur); }
    let mut out: Vec<String> = Vec::new();
    for cl in clauses {
        let core: String = cl.chars().filter(|c| !is_std_zh_punct(*c)).collect();
        let last_core: String = out.last().map(|o| o.chars().filter(|c| !is_std_zh_punct(*c)).collect()).unwrap_or_default();
        if core.chars().count() >= 2 && core == last_core {
            continue;
        }
        if core.chars().count() >= 3 && out.iter().any(|o| o.chars().filter(|c| !is_std_zh_punct(*c)).collect::<String>() == core) {
            continue;
        }
        out.push(cl);
    }
    let cap = audio_ms * 9 / 1000 + 6;
    let mut res = String::new();
    let mut n = 0usize;
    for cl in out {
        let k = cl.chars().filter(|c| !is_std_zh_punct(*c)).count();
        if n + k > cap && n > 0 { break; }
        n += k;
        res.push_str(&cl);
    }
    res
}
/// User-editable plain-text hotword list (`voice_hotwords.txt`, one term per line, `#` = comment).
pub fn load_hotword_file(dirs: &crate::config::Dirs) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(p) = dirs.newest("voice_hotwords.txt") {
        if let Ok(t) = std::fs::read_to_string(p) {
            for l in t.trim_start_matches('\u{feff}').lines() {
                let l = l.trim();
                if l.is_empty() || l.starts_with('#') || l.chars().count() > 16 { continue; }
                if !out.iter().any(|x: &String| x == l) { out.push(l.to_string()); }
            }
        }
    }
    out
}

pub fn merged_hotwords(dirs: &crate::config::Dirs) -> Vec<String> {
    let mut v = load_hotword_file(dirs);
    for w in dirs.load_custom_hotwords() {
        if !v.contains(&w) { v.push(w); }
    }
    v.truncate(40);
    v
}pub fn normalize_tail(tail: &str) -> String {
    let mut o = String::with_capacity(tail.len());
    for c in sanitize_punct(tail).chars() {
        if is_cjk(c) || c.is_ascii_alphanumeric() || c == '%' || c == '.' {
            o.push(c);
        } else if matches!(c, '。' | '？' | '！') {
            o.push(c);
        } else if c == ' ' {
            o.push(' ');
        } else {
            o.push('，');
        }
    }
    collapse_punct(&o)
}

/// Fullwidth Latin letters / stray colons used by Whisper as clause separators between CJK runs.
pub fn fix_separator_artifacts(s: &str) -> String {
    let ch: Vec<char> = s.chars().collect();
    let colons = ch.iter().filter(|c| **c == '：').count();
    let commas = ch.iter().filter(|c| **c == '，').count();
    let mut o = String::with_capacity(s.len());
    for (i, &c) in ch.iter().enumerate() {
        let prev_cjk = i > 0 && is_cjk(ch[i - 1]);
        let next_cjk_or_end = ch.get(i + 1).map(|n| is_cjk(*n) || is_std_zh_punct(*n)).unwrap_or(true);
        let fw_latin = matches!(c as u32, 0xFF21..=0xFF3A | 0xFF41..=0xFF5A);
        if fw_latin && prev_cjk && next_cjk_or_end {
            o.push('，');
        } else if c == '，' && i > 0 && ch[i - 1] == '%' && ch.get(i + 1).map(|n| matches!(*n, '到' | '至' | '的' | '左' | '以' | '上' | '多' | '和' | '或')).unwrap_or(false) {
            continue;
        } else if c == '：' && colons >= 3 && colons > commas {
            o.push('，');
        } else {
            o.push(c);
        }
    }
    collapse_punct(&o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixes_user_spontaneous_dictation_errors_and_segment_gluing() {
        let raw = "现在这个原因是识别工具做了一些提升和改进。但是我感觉，学用的时候还是不是太好。\n你看我现在发作，就是语音识别\n后的文字。根本就是不能直接使用。";
        let out = postprocess_v2(raw, None, true);
        assert!(out.contains("语音识别工具"), "got: {}", out);
        assert!(out.contains("使用的时候"), "got: {}", out);
        assert!(out.contains("我现在发出的，就是语音识别后的文字"), "got: {}", out);
        assert!(!out.contains("语音识别，后的文字"), "got: {}", out);
    }

    #[test]
    fn guard_accept_allows_multi_homophone_and_hesitation_repairs() {
        let inp = "现在这个原因是识别工具做了一些提升和改进。但是我感觉，学用的时候还是不是太好。";
        let llm = "现在这个语音识别工具做了一些提升和改进，但我感觉使用的时候还是不是太好。";
        assert!(guard_accept(inp, llm));

        let inp2 = "你看看有没有什么办法，不过让他，来，起码哎，我问。识别的正确度，然后您看能够上来。";
        let llm2 = "你看看有没有什么办法，不过起码让识别的正确度能够上来。";
        assert!(guard_accept(inp2, llm2));
    }
}
