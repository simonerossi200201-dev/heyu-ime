//! heyu-polish.exe — OPTIONAL helper for HEYU v2 voice dictation.
//! The only HEYU component that opens a socket, and only to Ollama on 127.0.0.1:11434 (hard-coded
//! loopback address; no DNS, no other host can be configured). heyu-voice.exe talks to it over
//! stdin/stdout so the voice engine itself keeps zero network imports. Delete this file to disable
//! LLM polish completely.
//! Protocol (one line each way):  `W\t<model>` -> `OK` | `ERR\t...`
//!                                `P\t<model>\t<timeout_ms>\t<context>\t<text>` -> `OK\t<polished>` | `ERR\t...`
#![windows_subsystem = "windows"]

use std::io::{BufRead, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// Extract a top-level string field from a flat JSON object (handles escapes incl. \uXXXX + surrogates).
fn json_string_field(body: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\":", key);
    let start = body.find(&pat)? + pat.len();
    let rest = body[start..].trim_start();
    let mut it = rest.strip_prefix('"')?.chars();
    let mut out = String::new();
    let mut pending_hi: Option<u32> = None;
    while let Some(c) = it.next() {
        match c {
            '"' => return Some(out),
            '\\' => {
                let e = it.next()?;
                match e {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => {}
                    'u' => {
                        let hex: String = (0..4).filter_map(|_| it.next()).collect();
                        let cp = u32::from_str_radix(&hex, 16).ok()?;
                        if (0xD800..0xDC00).contains(&cp) {
                            pending_hi = Some(cp);
                        } else if (0xDC00..0xE000).contains(&cp) {
                            if let Some(hi) = pending_hi.take() {
                                if let Some(ch) = char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (cp - 0xDC00)) { out.push(ch); }
                            }
                        } else if let Some(ch) = char::from_u32(cp) {
                            out.push(ch);
                        }
                    }
                    other => out.push(other),
                }
            }
            c => out.push(c),
        }
    }
    None
}

fn http_post_loopback(path: &str, body: &str, timeout: Duration) -> Result<String, String> {
    let addr: SocketAddr = "127.0.0.1:11434".parse().map_err(|_| "addr".to_string())?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(150)).map_err(|e| format!("connect: {}", e))?;
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(Duration::from_millis(300)));
    let _ = s.set_nodelay(true);
    let req = format!(
        "POST {} HTTP/1.1\r\nHost: 127.0.0.1:11434\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path,
        body.len(),
        body
    );
    s.write_all(req.as_bytes()).map_err(|e| format!("write: {}", e))?;
    // Read until the body is complete (Content-Length or chunked terminator); do not rely on close.
    let deadline = Instant::now() + timeout;
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        if Instant::now() > deadline { return Err("timeout".into()); }
        match s.read(&mut tmp) {
            Ok(0) => break,
            Ok(k) => buf.extend_from_slice(&tmp[..k]),
            Err(e) => return Err(format!("read: {}", e)),
        }
        if let Some(h) = find_sub(&buf, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..h]).to_ascii_lowercase();
            let body = &buf[h + 4..];
            if let Some(cl) = head.lines().find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())) {
                if body.len() >= cl { break; }
            } else if head.contains("transfer-encoding: chunked") && find_sub(body, b"0\r\n\r\n").is_some() {
                break;
            }
        }
    }
    let Some(h) = find_sub(&buf, b"\r\n\r\n") else { return Err("bad http".into()) };
    let head = String::from_utf8_lossy(&buf[..h]).to_string();
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return Err("http status".into());
    }
    let mut body = buf[h + 4..].to_vec();
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        let mut out = Vec::new();
        let mut i = 0usize;
        while let Some(e) = find_sub(&body[i..], b"\r\n") {
            let sz = usize::from_str_radix(String::from_utf8_lossy(&body[i..i + e]).trim(), 16).unwrap_or(0);
            if sz == 0 { break; }
            let st = i + e + 2;
            if st + sz > body.len() { break; }
            out.extend_from_slice(&body[st..st + sz]);
            i = st + sz + 2;
            if i >= body.len() { break; }
        }
        body = out;
    }
    Ok(String::from_utf8_lossy(&body).to_string())
}



fn find_sub(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() || h.len() < n.len() { return None; }
    (0..=h.len() - n.len()).find(|&i| &h[i..i + n.len()] == n)
}

const SYS_PROMPT: &str = "你是中文语音输入法的实时听写校对器。用户输入是麦克风语音识别的原始初稿，可能包含：1）同音或近音错别字（例如把“语音识别”误听成“原因是识别”、把“使用的时候”误听成“学用的时候”、把“我现在发出的”误听成“我现在发作”、把“十分钟读经”误听成“十分，在中毒经”）；2）说话思考卡顿产生的无意义碎词、口头禅和断句错误（例如“不过让他，来，起码哎，我问。识别的正确度，然后您看能够上来”应清理卡顿碎词并顺畅连接为“不过起码让识别的正确度能够上来”；“就是语音识别，后的文字”应去掉中间多余逗号为“就是语音识别后的文字”）。请执行校对：修正所有同音/近音错别字，仅删去无意义的卡顿口头禅和碎词（切勿删减有实际语义的完整分句，如“学习变成了工作”必须保留），修正标点符号与断句，使其成为自然通顺、可直接发送的中文。数字和百分比用阿拉伯数字。严格保留说话人的原意与语气，严禁回答问题、严禁扩写或解释，只输出校对后的正文。";


fn polish(model: &str, timeout_ms: u64, hotwords: &str, ctx_tail: &str, text: &str) -> Result<String, String> {
    let mut sys = String::from(SYS_PROMPT);
    if !hotwords.trim().is_empty() {
        sys.push_str("参考热词表：");
        sys.push_str(hotwords.trim());
        sys.push('。');
    }
    let user = if ctx_tail.trim().is_empty() {
        format!("待校对：{}", text)
    } else {
        format!("上文（仅供参考，不要输出）：{}\n待校对：{}", ctx_tail, text)
    };
    let np = (text.chars().count() * 2 + 16).min(400);
    let body = format!(
        "{{\"model\":\"{}\",\"system\":\"{}\",\"prompt\":\"{}\",\"stream\":false,\"think\":false,\"keep_alive\":\"60m\",\"options\":{{\"temperature\":0,\"num_ctx\":1024,\"num_predict\":{}}}}}",
        json_escape(model), json_escape(&sys), json_escape(&user), np
    );
    let resp = http_post_loopback("/api/generate", &body, Duration::from_millis(timeout_ms.clamp(300, 10_000)))?;
    json_string_field(&resp, "response").ok_or_else(|| "no response field".to_string())
}

fn warm(model: &str) -> Result<(), String> {
    let body = format!("{{\"model\":\"{}\",\"prompt\":\"你好\",\"stream\":false,\"think\":false,\"keep_alive\":\"60m\",\"options\":{{\"temperature\":0,\"num_ctx\":1024,\"num_predict\":1}}}}", json_escape(model));
    http_post_loopback("/api/generate", &body, Duration::from_millis(90_000)).map(|_| ())
}

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let f: Vec<&str> = line.splitn(6, '\t').collect();
        let reply = match f.as_slice() {
            ["W", model] => match warm(model) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR\t{}", e) },
            ["P", model, t, hw, ctx, text] => match polish(model, t.parse().unwrap_or(2400), hw, ctx, text) {
                Ok(r) => format!("OK\t{}", r.replace(['\n', '\r', '\t'], " ")),
                Err(e) => format!("ERR\t{}", e.replace(['\n', '\r', '\t'], " ")),
            },
            ["P", model, t, ctx, text] => match polish(model, t.parse().unwrap_or(2400), "", ctx, text) {
                Ok(r) => format!("OK\t{}", r.replace(['\n', '\r', '\t'], " ")),
                Err(e) => format!("ERR\t{}", e.replace(['\n', '\r', '\t'], " ")),
            },
            _ => "ERR\tbad request".to_string(),
        };
        if out.write_all(reply.as_bytes()).is_err() || out.write_all(b"\n").is_err() || out.flush().is_err() {
            break;
        }
    }
}
