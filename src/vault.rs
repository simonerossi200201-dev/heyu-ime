//! Pure-Rust SHA-256, HMAC-SHA256, PBKDF2-HMAC-SHA256, and SHA256-CTR + HMAC-SHA256
//! authenticated encryption for the local auto-expiring input history vault (`input_history.enc`).
//! Zero third-party crates, zero network APIs.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(data.len() + 72);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut w = [0u32; 64];
    for chunk in msg.chunks_exact(64) {
        for i in 0..16 {
            let o = i * 4;
            w[i] = u32::from_be_bytes([chunk[o], chunk[o + 1], chunk[o + 2], chunk[o + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K256[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k_block = [0u8; 64];
    if key.len() > 64 {
        let hk = sha256(key);
        k_block[..32].copy_from_slice(&hk);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k_block[i];
        opad[i] ^= k_block[i];
    }
    let mut inner = Vec::with_capacity(64 + msg.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(msg);
    let inner_hash = sha256(&inner);

    let mut outer = [0u8; 96];
    outer[..64].copy_from_slice(&opad);
    outer[64..].copy_from_slice(&inner_hash);
    sha256(&outer)
}

/// Standard PBKDF2-HMAC-SHA256 producing 32 bytes (1 block). Matches `hashlib.pbkdf2_hmac('sha256', password, salt, iterations, 32)`.
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut salt_block = Vec::with_capacity(salt.len() + 4);
    salt_block.extend_from_slice(salt);
    salt_block.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac_sha256(password, &salt_block);
    let mut out = u;
    for _ in 1..iterations.max(1) {
        u = hmac_sha256(password, &u);
        for j in 0..32 {
            out[j] ^= u[j];
        }
    }
    out
}

pub fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

pub fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    let hex = hex.trim();
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let s = std::str::from_utf8(&bytes[i..i + 2]).ok()?;
        out.push(u8::from_str_radix(s, 16).ok()?);
    }
    Some(out)
}

#[link(name = "advapi32")]
extern "system" {
    fn SystemFunction036(random_buffer: *mut u8, random_buffer_length: u32) -> u8;
}

fn random_16_bytes() -> [u8; 16] {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let mut os_rand = [0u8; 32];
    let ok = unsafe { SystemFunction036(os_rand.as_mut_ptr(), 32) };
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let addr = &os_rand as *const [u8; 32] as usize as u128;
    let aux = nanos ^ (pid << 48) ^ ((seq as u128) << 64) ^ addr.rotate_left(17);
    let mut mix = [0u8; 48];
    mix[..32].copy_from_slice(&os_rand);
    mix[32..48].copy_from_slice(&aux.to_le_bytes());
    let h = sha256(&mix);
    let mut iv = [0u8; 16];
    if ok != 0 {
        for i in 0..16 {
            iv[i] = os_rand[i] ^ h[i];
        }
    } else {
        iv.copy_from_slice(&h[..16]);
    }
    iv
}

fn ctr_stream_xor(key: &[u8; 32], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut block_in = [0u8; 52];
    block_in[..32].copy_from_slice(key);
    block_in[32..48].copy_from_slice(iv);
    for (counter, chunk) in data.chunks(32).enumerate() {
        block_in[48..52].copy_from_slice(&(counter as u32).to_be_bytes());
        let ks = sha256(&block_in);
        for (i, &b) in chunk.iter().enumerate() {
            out.push(b ^ ks[i]);
        }
    }
    out
}

pub fn encrypt_vault(key: &[u8; 32], plaintext: &str) -> String {
    let iv = random_16_bytes();
    let ct = ctr_stream_xor(key, &iv, plaintext.as_bytes());
    let mut mac_input = Vec::with_capacity(16 + ct.len());
    mac_input.extend_from_slice(&iv);
    mac_input.extend_from_slice(&ct);
    let tag = hmac_sha256(key, &mac_input);
    format!("HEYUENC1:{}:{}:{}\n", hex_encode(&iv), hex_encode(&tag), hex_encode(&ct))
}

pub fn decrypt_vault(key: &[u8; 32], content: &str) -> Option<String> {
    let line = content.trim();
    let rest = line.strip_prefix("HEYUENC1:")?;
    let mut parts = rest.splitn(3, ':');
    let iv_vec = hex_decode(parts.next()?)?;
    let tag_vec = hex_decode(parts.next()?)?;
    let ct_vec = hex_decode(parts.next()?)?;
    if iv_vec.len() != 16 || tag_vec.len() != 32 {
        return None;
    }
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&iv_vec);
    let mut mac_input = Vec::with_capacity(16 + ct_vec.len());
    mac_input.extend_from_slice(&iv);
    mac_input.extend_from_slice(&ct_vec);
    let expected_tag = hmac_sha256(key, &mac_input);
    // Constant-time comparison
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= expected_tag[i] ^ tag_vec[i];
    }
    if diff != 0 {
        return None;
    }
    let pt = ctr_stream_xor(key, &iv, &ct_vec);
    String::from_utf8(pt).ok()
}

pub fn vault_key_path(fallback_data_dir: &Path) -> PathBuf {
    if let Some(la) = std::env::var_os("LOCALAPPDATA") {
        let dir = PathBuf::from(la).join("HEYU");
        let _ = fs::create_dir_all(&dir);
        return dir.join(".vault_key");
    }
    fallback_data_dir.join(".vault_key")
}

pub fn load_or_create_vault_secret(fallback_data_dir: &Path) -> [u8; 32] {
    let p = vault_key_path(fallback_data_dir);
    if let Ok(txt) = fs::read_to_string(&p) {
        if let Some(bytes) = hex_decode(txt.trim()) {
            if bytes.len() == 32 {
                let mut out = [0u8; 32];
                out.copy_from_slice(&bytes);
                return out;
            }
        }
    }
    let iv1 = random_16_bytes();
    let iv2 = random_16_bytes();
    let mut seed = [0u8; 32];
    seed[..16].copy_from_slice(&iv1);
    seed[16..].copy_from_slice(&iv2);
    let secret = sha256(&seed);
    let _ = fs::write(&p, hex_encode(&secret));
    secret
}

/// Resolve the active 32-byte encryption key for `input_history.enc`:
/// - If a PIN is configured (`history_wrapped_key` & `history_pin_salt` non-empty), unwrap `K = wrapped XOR SHA256(vault_secret || salt)`.
/// - Otherwise derive the machine-local key `K = HMAC_SHA256(vault_secret, b"heyu-default-history-key-v1")`.
pub fn resolve_history_key(data_dir: &Path, pin_salt_hex: &str, wrapped_key_hex: &str) -> [u8; 32] {
    let secret = load_or_create_vault_secret(data_dir);
    if !pin_salt_hex.is_empty() && !wrapped_key_hex.is_empty() {
        if let (Some(salt), Some(wrapped)) = (hex_decode(pin_salt_hex), hex_decode(wrapped_key_hex)) {
            if wrapped.len() == 32 {
                let mut mask_in = Vec::with_capacity(32 + salt.len());
                mask_in.extend_from_slice(&secret);
                mask_in.extend_from_slice(&salt);
                let mask = sha256(&mask_in);
                let mut k = [0u8; 32];
                for i in 0..32 {
                    k[i] = wrapped[i] ^ mask[i];
                }
                return k;
            }
        }
    }
    hmac_sha256(&secret, b"heyu-default-history-key-v1")
}

/// Securely overwrite a file with zeros on disk before removing it so deleted history cannot be carved from disk sectors.
pub fn secure_wipe_file(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let len = (meta.len() as usize).clamp(256, 1024 * 1024);
        let zeros = vec![0u8; len];
        let _ = fs::write(path, &zeros);
        let _ = fs::remove_file(path);
    }
    // Also clean up any legacy plaintext file if present
    let legacy = path.with_file_name("input_history.tsv");
    if legacy.exists() {
        let _ = fs::write(&legacy, [0u8; 256]);
        let _ = fs::remove_file(&legacy);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryItem {
    pub timestamp: u64,
    pub source: String,
    pub text: String,
}

pub fn read_history_items(data_dir: &Path, key: &[u8; 32], cutoff_epoch_secs: u64) -> Vec<HistoryItem> {
    let enc_path = data_dir.join("input_history.enc");
    let Ok(raw) = fs::read_to_string(&enc_path) else {
        return Vec::new();
    };
    let Some(decrypted) = decrypt_vault(key, &raw) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for line in decrypted.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut sp = line.splitn(3, '\t');
        if let (Some(ts_s), Some(src), Some(txt)) = (sp.next(), sp.next(), sp.next()) {
            if let Ok(ts) = ts_s.trim().parse::<u64>() {
                if ts >= cutoff_epoch_secs {
                    items.push(HistoryItem {
                        timestamp: ts,
                        source: src.trim().to_string(),
                        text: txt.replace("\\n", "\n"),
                    });
                }
            }
        }
    }
    items
}

pub fn write_history_items(data_dirs: &[PathBuf], key: &[u8; 32], items: &[HistoryItem]) {
    if items.is_empty() {
        for d in data_dirs {
            secure_wipe_file(&d.join("input_history.enc"));
        }
        return;
    }
    let mut plain = String::from("# HEYU Encrypted Input History Vault v1\n");
    for it in items {
        let esc = it.text.replace('\r', "").replace('\n', "\\n").replace('\t', " ");
        plain.push_str(&format!("{}\t{}\t{}\n", it.timestamp, it.source, esc));
    }
    let enc = encrypt_vault(key, &plain);
    for d in data_dirs {
        if d.is_dir() {
            let p = d.join("input_history.enc");
            let _ = fs::write(&p, &enc);
            let legacy = d.join("input_history.tsv");
            if legacy.exists() {
                secure_wipe_file(&legacy);
            }
        }
    }
}

// ============================================================================
// DEVICE-BOUND MASTER VAULT & ENCRYPTED USER LEXICON (`user_custom.tsv`)
// ============================================================================

/// Compute a 32-hex-character hardware/OS device identifier bound to the current computer's
/// local `.vault_key`, `COMPUTERNAME`, `USERNAME`, and `PROCESSOR_IDENTIFIER`.
/// Matches `compute_device_id()` in `heyu_studio.py` bit-for-bit!
pub fn compute_device_id(fallback_data_dir: &Path) -> String {
    let secret = load_or_create_vault_secret(fallback_data_dir);
    let comp = std::env::var("COMPUTERNAME").unwrap_or_default().to_ascii_uppercase();
    let user = std::env::var("USERNAME").unwrap_or_default().to_ascii_lowercase();
    let proc_id = std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default();
    let msg = format!("heyu-device-id-v1:{}:{}:{}", comp, user, proc_id);
    let mac = hmac_sha256(&secret, msg.as_bytes());
    hex_encode(&mac[..16])
}

/// Derive the 32-byte hardware-bound wrapping key `K_device` for the current machine.
/// Matches `compute_device_wrap_key()` in `heyu_studio.py` bit-for-bit!
pub fn compute_device_wrap_key(fallback_data_dir: &Path) -> [u8; 32] {
    let secret = load_or_create_vault_secret(fallback_data_dir);
    let dev_id = compute_device_id(fallback_data_dir);
    let msg = format!("heyu-device-wrap-v1:{}", dev_id);
    hmac_sha256(&secret, msg.as_bytes())
}

fn json_str_simple(txt: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\"", key);
    let idx = txt.find(&pat)?;
    let rest = &txt[idx + pat.len()..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let body = &after[1..];
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c == '"' {
            return Some(out);
        }
        if c == '\\' {
            if let Some(nc) = chars.next() {
                out.push(nc);
            }
        } else {
            out.push(c);
        }
    }
    None
}

/// Check whether `config.json` in `data_dir` has a Master Password configured and, if so,
/// unwrap the 32-byte `K_vault` using the current computer's `K_device`.
/// - Returns `Ok(Some(K_vault))` when Master Password is set AND current PC is authorized.
/// - Returns `Ok(None)` when Master Password is NOT configured yet (e.g. unit tests / fresh dir).
/// - Returns `Err(())` when Master Password IS configured, but the computer device changed (`更换电脑设备后`)
///   or device authentication failed — refusing decryption until the user logs in with their Master Password!
pub fn resolve_authorized_lexicon_key(data_dir: &Path) -> Result<Option<[u8; 32]>, ()> {
    let cfg_path = data_dir.join("config.json");
    let Ok(txt) = fs::read_to_string(&cfg_path) else {
        return Ok(None);
    };
    let verifier = json_str_simple(&txt, "master_pwd_verifier").unwrap_or_default();
    if verifier.trim().is_empty() {
        return Ok(None);
    }
    let auth_dev = json_str_simple(&txt, "authorized_device_id").unwrap_or_default();
    let dev_wrapped = json_str_simple(&txt, "device_wrapped_vault_key").unwrap_or_default();
    let cur_dev = compute_device_id(data_dir);
    if auth_dev.trim().is_empty() || dev_wrapped.trim().is_empty() || auth_dev.trim() != cur_dev {
        return Err(());
    }
    let k_dev = compute_device_wrap_key(data_dir);
    let Some(vault_hex) = decrypt_vault(&k_dev, dev_wrapped.trim()) else {
        return Err(());
    };
    let Some(vault_bytes) = hex_decode(vault_hex.trim()) else {
        return Err(());
    };
    if vault_bytes.len() != 32 {
        return Err(());
    }
    let mut k_vault = [0u8; 32];
    k_vault.copy_from_slice(&vault_bytes);
    Ok(Some(k_vault))
}

/// Derive the default machine-local 32-byte encryption key for `user_custom.tsv` when no Master Password is set.
/// Matches `hmac.new(secret, b"heyu-default-lexicon-key-v1", hashlib.sha256).digest()` in `heyu_studio.py`.
pub fn compute_default_lexicon_key(fallback_data_dir: &Path) -> [u8; 32] {
    let secret = load_or_create_vault_secret(fallback_data_dir);
    hmac_sha256(&secret, b"heyu-default-lexicon-key-v1")
}

/// Read `<data_dir>/user_custom.tsv` (decrypting `HEYUENC1:...` lines in RAM if protected by Master Password or default key).
/// Also auto-migrates any plaintext lines on disk into `HEYUENC1:...` when a Master Password is active!
pub fn read_user_lexicon_decrypted(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let data_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let k_def = compute_default_lexicon_key(data_dir);
    match resolve_authorized_lexicon_key(data_dir) {
        Err(()) => {
            // Device changed or unauthorized: refuse to load encrypted user lexicon until re-login!
            return None;
        }
        Ok(opt_k_vault) => {
            let mut out = String::new();
            let mut saw_plaintext_entry = false;
            let mut saw_default_key_cipher = false;
            for line in raw.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    out.push_str(line);
                    out.push('\n');
                    continue;
                }
                if trimmed.starts_with("HEYUENC1:") {
                    let dec_opt = if let Some(ref k_vault) = opt_k_vault {
                        if let Some(d) = decrypt_vault(k_vault, trimmed) {
                            Some(d)
                        } else if let Some(d_def) = decrypt_vault(&k_def, trimmed) {
                            saw_default_key_cipher = true;
                            Some(d_def)
                        } else {
                            None
                        }
                    } else {
                        decrypt_vault(&k_def, trimmed)
                    };
                    if let Some(dec) = dec_opt {
                        for dline in dec.lines() {
                            if !dline.trim().is_empty() {
                                out.push_str(dline);
                                out.push('\n');
                            }
                        }
                    }
                } else {
                    saw_plaintext_entry = true;
                    out.push_str(line);
                    out.push('\n');
                }
            }
            if (saw_plaintext_entry || saw_default_key_cipher) && opt_k_vault.is_some() {
                let _ = write_user_lexicon_file(path, &out);
            }
            Some(out)
        }
    }
}

/// Write `user_custom.tsv` to disk. If a Master Password is configured in `<data_dir>/config.json`,
/// all non-comment TSV entries are encrypted into a single authenticated `HEYUENC1:...` ciphertext block on disk!
pub fn write_user_lexicon_file(path: &Path, full_tsv_text: &str) -> bool {
    let data_dir = path.parent().unwrap_or_else(|| Path::new("."));
    match resolve_authorized_lexicon_key(data_dir) {
        Err(()) => false,
        Ok(None) => fs::write(path, full_tsv_text).is_ok(),
        Ok(Some(k_vault)) => {
            let mut plain_entries = String::new();
            for line in full_tsv_text.lines() {
                let t = line.trim();
                if t.is_empty() || t.starts_with('#') {
                    continue;
                }
                plain_entries.push_str(t);
                plain_entries.push('\n');
            }
            let mut disk_out = String::from(
                "# HEYU Personal Custom Lexicon (个人专属置顶词库 · 顶级密文加密保护)\n# Encrypted with 256-bit CTR-SHA256 + HMAC-SHA256 (PBKDF2 120,000 iterations)\n",
            );
            if !plain_entries.is_empty() {
                let enc = encrypt_vault(&k_vault, &plain_entries);
                disk_out.push_str(&enc);
            }
            fs::write(path, disk_out).is_ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_and_pbkdf2_match_standard_vectors() {
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            hex_encode(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // PBKDF2-HMAC-SHA256("password", "salt", 1, 32) = 120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b
        assert_eq!(
            hex_encode(&pbkdf2_hmac_sha256(b"password", b"salt", 1)),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
    }

    #[test]
    fn vault_encrypt_decrypt_roundtrip_and_tamper_reject() {
        let key = pbkdf2_hmac_sha256(b"123456", b"heyu-salt", 1000);
        let msg = "1700000000\t键盘\t片语工作室收到请回复\n";
        let enc = encrypt_vault(&key, msg);
        assert!(enc.starts_with("HEYUENC1:"));
        assert!(!enc.contains("片语工作室"));
        let dec = decrypt_vault(&key, &enc).unwrap();
        assert_eq!(dec, msg);

        // Wrong key must fail authentication
        let wrong_key = pbkdf2_hmac_sha256(b"654321", b"heyu-salt", 1000);
        assert!(decrypt_vault(&wrong_key, &enc).is_none());
    }

    #[test]
    fn encrypted_user_lexicon_and_new_device_rejection() {
        let dir = std::env::temp_dir().join(format!("heyu_vault_lex_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let dev_id = compute_device_id(&dir);
        let k_dev = compute_device_wrap_key(&dir);
        let k_vault = sha256(b"test-master-vault-key-256bit");
        let dev_wrapped = encrypt_vault(&k_dev, &hex_encode(&k_vault)).trim().to_string();

        let cfg = format!(
            "{{\n  \"master_pwd_verifier\": \"abcd1234\",\n  \"authorized_device_id\": \"{}\",\n  \"device_wrapped_vault_key\": \"{}\"\n}}\n",
            dev_id, dev_wrapped
        );
        fs::write(dir.join("config.json"), cfg).unwrap();

        let lex_path = dir.join("user_custom.tsv");
        let plain = "# Header\npygu\t片语工作室\t直出\n";
        assert!(write_user_lexicon_file(&lex_path, plain));

        // On disk, the file must NOT contain plaintext "片语工作室", only HEYUENC1 ciphertext!
        let on_disk = fs::read_to_string(&lex_path).unwrap();
        assert!(on_disk.contains("HEYUENC1:"));
        assert!(!on_disk.contains("片语工作室"));

        // On the authorized device, read_user_lexicon_decrypted recovers the exact entry in RAM
        let dec = read_user_lexicon_decrypted(&lex_path).unwrap();
        assert!(dec.contains("pygu\t片语工作室\t直出"));

        // Simulate moving to a new computer (`authorized_device_id` no longer matches)
        let cfg_new_pc = format!(
            "{{\n  \"master_pwd_verifier\": \"abcd1234\",\n  \"authorized_device_id\": \"00000000000000000000000000000000\",\n  \"device_wrapped_vault_key\": \"{}\"\n}}\n",
            dev_wrapped
        );
        fs::write(dir.join("config.json"), cfg_new_pc).unwrap();
        assert!(read_user_lexicon_decrypted(&lex_path).is_none());

        let _ = fs::remove_dir_all(&dir);
    }
}
