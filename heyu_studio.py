#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
HEYU STUDIO · 鹤羽音形设置中心
==============================
Companion settings app for the native Rust `heyu-ime.exe` input engine.

Layout: left sidebar navigation + right content area made of grouped "setting rows"
(label + one-line description on the left, control on the right), ordered by how often
each setting is changed.

Performance rules (the old tabbed UI took ~6.5 s to open):
  * only the first page is built at start-up; every other page is built on first visit
    and cached (switching = pack_forget / pack, never rebuild);
  * heavy data (reverse dictionary, short codes, root atlas, theme previews, PE audit)
    is loaded on demand in a background thread and handed back via after();
  * long lists use ttk.Treeview instead of hundreds of CustomTkinter widgets;
  * engine status is read through the Toolhelp snapshot API (no `tasklist` subprocess).

CLI:
  heyu_studio.py --page <general|dict|voice|appearance|shortcuts|practice|about>
Single instance: a second launch forwards the requested page to the running window
(named mutex + request file + FindWindow) and exits. No sockets / network APIs.
"""

import os
import sys
import json
import time
import struct
import hashlib
import hmac
import secrets
import shutil
import winsound
import threading
import subprocess
import ctypes
from ctypes import wintypes
import tkinter as tk
import tkinter.font as tkfont
from tkinter import ttk, messagebox, filedialog

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
LOCAL_DATA_DIR = os.path.join(BASE_DIR, "data")
APPDATA_ROOT = os.path.join(os.environ["APPDATA"], "HEYU") if os.environ.get("APPDATA") else ""
APPDATA_DATA_DIR = os.path.join(APPDATA_ROOT, "data") if APPDATA_ROOT else ""
INSTALL_DIR = r"C:\Program Files\HEYU"
IS_PORTABLE = os.path.exists(os.path.join(BASE_DIR, "portable.flag"))

if not IS_PORTABLE and (os.path.exists(os.path.join(BASE_DIR, "heyu_installed.marker")) or os.path.exists(os.path.join(INSTALL_DIR, "heyu_installed.marker"))) and APPDATA_DATA_DIR:
    DATA_DIR = APPDATA_DATA_DIR
    os.makedirs(DATA_DIR, exist_ok=True)
else:
    DATA_DIR = LOCAL_DATA_DIR
    os.makedirs(DATA_DIR, exist_ok=True)

SYNC_DATA_DIRS = []
if IS_PORTABLE:
    SYNC_DATA_DIRS = [LOCAL_DATA_DIR]
else:
    for _d in (LOCAL_DATA_DIR, APPDATA_DATA_DIR):
        if _d and _d not in SYNC_DATA_DIRS and (os.path.exists(_d) or _d == DATA_DIR):
            SYNC_DATA_DIRS.append(_d)

ASSETS_DIR = os.path.join(BASE_DIR, "assets")
EXE_PATH = os.path.join(BASE_DIR, "heyu-ime.exe") if (IS_PORTABLE or os.path.exists(os.path.join(BASE_DIR, "heyu-ime.exe"))) else os.path.join(INSTALL_DIR, "heyu-ime.exe")
VOICE_EXE_PATH = os.path.join(BASE_DIR, "heyu-voice.exe") if (IS_PORTABLE or os.path.exists(os.path.join(BASE_DIR, "heyu-voice.exe"))) else os.path.join(INSTALL_DIR, "heyu-voice.exe")
CONFIG_PATH = os.path.join(DATA_DIR, "config.json")
TELEMETRY_PATH = os.path.join(DATA_DIR, "telemetry.json")
USER_DICT_PATH = os.path.join(DATA_DIR, "user_custom.tsv")
REVERSE_PATH = os.path.join(LOCAL_DATA_DIR, "heyu_reverse.tsv") if os.path.exists(os.path.join(LOCAL_DATA_DIR, "heyu_reverse.tsv")) else os.path.join(DATA_DIR, "heyu_reverse.tsv")
SYS_DICT_PATH = os.path.join(LOCAL_DATA_DIR, "heyu_sys.tsv") if os.path.exists(os.path.join(LOCAL_DATA_DIR, "heyu_sys.tsv")) else os.path.join(DATA_DIR, "heyu_sys.tsv")
STUDIO_UI_PATH = os.path.join(DATA_DIR, "studio_ui.json")   # Studio-only UI prefs (appearance)
HUD_STYLE_PATH = os.path.join(DATA_DIR, "hud_style.json")   # read by the Rust HUD
REQUEST_PATH = os.path.join(DATA_DIR, "studio_request.json")  # single-instance page hand-off
ICON_PATH = os.path.join(ASSETS_DIR, "heyu.ico")
PREVIEW_DIR = os.path.join(ASSETS_DIR, "previews")
THEMES_JSON_PATH = os.path.join(PREVIEW_DIR, "themes.json")
LOCAL_MODELS_DIR = os.path.join(BASE_DIR, "models")
if IS_PORTABLE or (os.path.isdir(LOCAL_MODELS_DIR) and any(f.endswith(".bin") for f in os.listdir(LOCAL_MODELS_DIR))):
    MODELS_DIR = LOCAL_MODELS_DIR
else:
    MODELS_DIR = os.path.join(APPDATA_ROOT, "models") if APPDATA_ROOT else LOCAL_MODELS_DIR
VOICE_CONFIG_NAME = "voice_config.json"

# Window title is also the FindWindow key for single-instance activation (kept stable).
WINDOW_TITLE = "鹤羽音形 HEYU YinXing · 控制中心"
PAGES = ("general", "dict", "voice", "appearance", "security", "shortcuts", "practice", "about")

# Ensure stdout/stderr work under pythonw.exe
LOG_FILE = os.path.join(DATA_DIR, "studio.log")
if sys.stdout is None:
    sys.stdout = open(LOG_FILE, "a", encoding="utf-8")
if sys.stderr is None:
    sys.stderr = open(LOG_FILE, "a", encoding="utf-8")

import customtkinter as ctk  # noqa: E402  (after the stdout fix so pythonw never crashes on prints)


# =============================================================================
# DESIGN TOKENS
# =============================================================================
PALETTES = {
    "light": {
        "bg": "#f5f6f8",
        "sidebar": "#eceef2",
        "nav_hover": "#e2e5ea",
        "nav_sel": "#dcdfe5",
        "surface": "#ffffff",
        "surface_alt": "#f5f6f8",
        "surface_sunken": "#eceef2",
        "border": "#e4e7ec",
        "border_strong": "#cfd4dc",
        "switch_off": "#c9ced6",
        "text": "#1c1f24",
        "text_2": "#4a5260",
        "muted": "#7b838f",
        "accent": "#b45309",
        "accent_hover": "#92400e",
        "accent_soft": "#fbefdd",
        "on_accent": "#ffffff",
        "success": "#047857",
        "success_soft": "#e2f4ec",
        "danger": "#c62828",
        "danger_hover": "#a31f1f",
        "danger_soft": "#fdeceb",
        "info": "#1d4ed8",
        "purple": "#7e22ce",
        "seg_bg": "#e8eaee",
        "seg_sel": "#ffffff",
        "chip": "#f1f3f6",
    },
    "dark": {
        "bg": "#16181d",
        "sidebar": "#111317",
        "nav_hover": "#1c1f26",
        "nav_sel": "#242832",
        "surface": "#1d2027",
        "surface_alt": "#22262e",
        "surface_sunken": "#16181d",
        "border": "#2a2e38",
        "border_strong": "#3a404d",
        "switch_off": "#3a404d",
        "text": "#e8eaee",
        "text_2": "#b0b7c3",
        "muted": "#7c8492",
        "accent": "#f5a524",
        "accent_hover": "#ffbe4d",
        "accent_soft": "#2f2617",
        "on_accent": "#1b1305",
        "success": "#34d399",
        "success_soft": "#16302a",
        "danger": "#ef5350",
        "danger_hover": "#f26f6c",
        "danger_soft": "#3a1d1e",
        "info": "#60a5fa",
        "purple": "#c084fc",
        "seg_bg": "#262a33",
        "seg_sel": "#3a404d",
        "chip": "#262a33",
    },
}


def C(name: str):
    """(light, dark) colour tuple — customtkinter switches it live with the appearance mode."""
    return (PALETTES["light"][name], PALETTES["dark"][name])


# 8px grid
RADIUS_CARD = 12
RADIUS_CTRL = 8
BTN_H = 34
ROW_PX = 20          # horizontal padding inside a group card
ROW_PY = 14          # vertical padding of a setting row
PAGE_PX = 32         # minimum page side padding
CONTENT_MAX_W = 820  # centred max content width

# Theme catalogue fallback (assets/previews/themes.json takes precedence when present).
FALLBACK_THEMES = [
    {"index": 0, "slug": "obsidian_gold", "name": "曜石流金 · Obsidian Gold", "is_dark": True, "accent": "#eeb24c", "bg": "#1c1d24", "art": ""},
    {"index": 1, "slug": "cyber_jade", "name": "翡翠极光 · Cyber Jade", "is_dark": True, "accent": "#34d399", "bg": "#132020", "art": ""},
    {"index": 2, "slug": "nebula_cyan", "name": "深海星云 · Nebula Cyan", "is_dark": True, "accent": "#5ecafa", "bg": "#141b2d", "art": ""},
    {"index": 3, "slug": "washi", "name": "和紙 · Washi", "is_dark": False, "accent": "#b84230", "bg": "#f6f0e2", "art": ""},
    {"index": 4, "slug": "macos_frost", "name": "霜白 · macOS Frost", "is_dark": False, "accent": "#007aff", "bg": "#f6f6f9", "art": ""},
    {"index": 5, "slug": "notion_paper", "name": "纸白 · Notion Paper", "is_dark": False, "accent": "#2383e2", "bg": "#ffffff", "art": ""},
    {"index": 6, "slug": "sakura", "name": "樱 · Sakura", "is_dark": False, "accent": "#d65680", "bg": "#fef6f8", "art": ""},
    {"index": 7, "slug": "mint", "name": "薄荷 · Mint", "is_dark": False, "accent": "#0e946a", "bg": "#f1faf6", "art": ""},
    {"index": 8, "slug": "graphite", "name": "石墨 · Graphite", "is_dark": True, "accent": "#0a84ff", "bg": "#29292d", "art": ""},
    {"index": 9, "slug": "crane_cloud", "name": "浮世白鹤 · Crane & Cloud", "is_dark": False, "accent": "#c23826", "bg": "#faf6ee", "art": "crane_cloud", "tag": "插画 · 朱印祥云白鹤"},
    {"index": 10, "slug": "bamboo_panda", "name": "幽谷翠竹 · Bamboo & Panda", "is_dark": False, "accent": "#2a7c4e", "bg": "#f0f7ee", "art": "bamboo_panda", "tag": "插画 · 翠竹水墨萌熊"},
    {"index": 11, "slug": "amber_cat", "name": "橘猫午后 · Amber Cat", "is_dark": False, "accent": "#d06622", "bg": "#fcf6ec", "art": "amber_cat", "tag": "插画 · 暖阳橘猫梅花爪"},
    {"index": 12, "slug": "camellia_flora", "name": "山茶花庭 · Camellia Garden", "is_dark": False, "accent": "#b83a52", "bg": "#fcf4f5", "art": "camellia_flora", "tag": "插画 · 法式复古山茶花"},
    {"index": 13, "slug": "starry_fox", "name": "星夜灵狐 · Starry Fox", "is_dark": True, "accent": "#f2bc58", "bg": "#15182c", "art": "starry_fox", "tag": "插画 · 星月流金灵狐"},
    {"index": 14, "slug": "forest_stag", "name": "深林鹿影 · Forest Stag", "is_dark": True, "accent": "#70daa6", "bg": "#111e1b", "art": "forest_stag", "tag": "插画 · 北欧松林流萤鹿影"},
]

VOICE_DEFAULTS = {
    "enabled": True,
    "hotkey": "Ctrl+Alt+V",
    "loopback_hotkey": "Ctrl+Alt+B",
    "audio_source": "mic",
    "target_pid": 0,
    "cursor_mode": "independent",
    "continuous_mode": True,
    "smart_correction": True,
    "language": "auto",
    "model": "large-v3-q5_0",
    "use_gpu": "auto",
    "auto_stop_silence_ms": 1200,
    "max_record_s": 1200,
    "simplified_chinese": True,
    "replace_rules": "",
    "idle_unload_mins": 30,
}
# (model id, display name, note, recommended)
VOICE_MODELS = [
    ("large-v3-q5_0", "Large v3 (全 32 层最高精度旗舰版)", "准确率 ★★★★★ · 速度 ★★★☆☆ · 约 1.03 GB · 完整 32 层解码器，新闻/口音/专业词汇最高精度", True),
    ("large-v3-turbo-q5_0", "Large v3 Turbo (高速高精均衡版)", "准确率 ★★★★☆ · 速度 ★★★★★ · 约 550 MB · 4 层轻量解码，CPU 笔记本亦极其流畅", False),
    ("large-v3", "Large v3 FP16 (无损母版)", "准确率 ★★★★★ · 速度 ★★☆☆☆ · 约 3.1 GB · 未量化 FP16 原版，适合大显存独立显卡", False),
    ("medium-q5_0", "Medium", "准确率 ★★★★☆ · 速度 ★★★☆☆ · 约 540 MB · 经典均衡模型", False),
    ("small-q5_1", "Small", "准确率 ★★★☆☆ · 速度 ★★★★☆ · 约 190 MB · 轻量快速", False),
    ("base-q5_1", "Base", "准确率 ★★☆☆☆ · 速度 ★★★★★ · 约 60 MB · 极速低内存", False),
]


def _read_json(path, default):
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return default


def _write_json(path, data):
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2, ensure_ascii=False)
    except Exception as e:
        print(f"[studio] failed to write {path}: {e}", file=sys.stderr)


# =============================================================================
# VAULT CRYPTO HELPERS (PBKDF2-HMAC-SHA256 + SHA256-CTR + HMAC-SHA256, interop with src/vault.rs)
# =============================================================================
def _vault_key_path(fallback_data_dir: str) -> str:
    la = os.environ.get("LOCALAPPDATA")
    if la:
        d = os.path.join(la, "HEYU")
        try:
            os.makedirs(d, exist_ok=True)
        except Exception:
            pass
        return os.path.join(d, ".vault_key")
    return os.path.join(fallback_data_dir, ".vault_key")


def _vault_load_secret(data_dir: str) -> bytes:
    key_path = _vault_key_path(data_dir)
    try:
        if os.path.exists(key_path):
            raw = open(key_path, "r", encoding="utf-8").read().strip()
            b = bytes.fromhex(raw)
            if len(b) == 32:
                return b
    except Exception:
        pass
    # Migrate legacy data_dir/.vault_key if present
    legacy_path = os.path.join(data_dir, ".vault_key")
    try:
        if legacy_path != key_path and os.path.exists(legacy_path):
            raw = open(legacy_path, "r", encoding="utf-8").read().strip()
            b = bytes.fromhex(raw)
            if len(b) == 32:
                with open(key_path, "w", encoding="utf-8") as f:
                    f.write(b.hex())
                return b
    except Exception:
        pass
    secret = secrets.token_bytes(32)
    try:
        os.makedirs(os.path.dirname(key_path), exist_ok=True)
        with open(key_path, "w", encoding="utf-8") as f:
            f.write(secret.hex())
        try:
            ctypes.windll.kernel32.SetFileAttributesW(str(key_path), 0x02)  # FILE_ATTRIBUTE_HIDDEN
        except Exception:
            pass
    except Exception:
        pass
    return secret


def _compute_device_id(data_dir: str) -> str:
    """Matches `src/vault.rs::compute_device_id` bit-for-bit."""
    secret = _vault_load_secret(data_dir)
    comp = os.environ.get("COMPUTERNAME", "").upper()
    user = os.environ.get("USERNAME", "").lower()
    proc_id = os.environ.get("PROCESSOR_IDENTIFIER", "")
    msg = f"heyu-device-id-v1:{comp}:{user}:{proc_id}".encode("utf-8")
    mac = hmac.new(secret, msg, hashlib.sha256).digest()
    return mac[:16].hex()


def _compute_device_wrap_key(data_dir: str) -> bytes:
    """Matches `src/vault.rs::compute_device_wrap_key` bit-for-bit."""
    secret = _vault_load_secret(data_dir)
    dev_id = _compute_device_id(data_dir)
    msg = f"heyu-device-wrap-v1:{dev_id}".encode("utf-8")
    return hmac.new(secret, msg, hashlib.sha256).digest()


MASTER_PBKDF2_ITERS = 120000
REMINDER_PASSWORD_SECS = 14 * 86400   # 14 days (2 weeks) mandatory password check
REMINDER_BACKUP_SECS = 30 * 86400     # 30 days (1 month) lexicon backup reminder


def _check_password_strength(pwd: str):
    """
    Cryptographic anti-brute-force password policy checker.
    Returns (is_valid: bool, score_0_to_4: int, status_text: str, checklist: list[(bool, str)]).
    """
    s = pwd or ""
    len_ok = len(s) >= 10
    has_upper = any(c.isupper() for c in s)
    has_lower = any(c.islower() for c in s)
    has_digit = any(c.isdigit() for c in s)
    has_sym = any((not c.isalnum()) and (not c.isspace()) for c in s)
    classes = sum([has_upper, has_lower, has_digit, has_sym])
    class_ok = classes >= 3

    low = s.lower()
    weak_subs = ("123456", "654321", "qwerty", "asdfgh", "password", "admin123", "111111", "000000", "aaaaaa")
    no_weak = (len(set(s)) >= 6) and not any(w in low for w in weak_subs)

    rules = [
        (len_ok, f"长度至少 10 位（当前 {len(s)} 位，推荐 12 位以上）"),
        (class_ok, f"包含大写、小写、数字、特殊符号中的至少 3 类（当前 {classes}/4 类）"),
        (no_weak, "至少 6 个不同字符，且不包含连续序列或常见弱口令特征"),
    ]
    valid = len_ok and class_ok and no_weak
    score = sum([len(s) >= 10, classes >= 2, classes >= 3 and no_weak, len(s) >= 13 and classes == 4 and no_weak])
    if not s:
        lbl = "待输入（需符合密码学抗暴力破解强度标准）"
    elif not valid:
        lbl = "⚠️ 强度不足（需至少 10 位，且含大小写字母/数字/特殊符号中至少 3 类，禁止连续重复弱口令）"
    elif score >= 4:
        lbl = "🛡️ 极强 · 军工级高熵抗暴力破解（PBKDF2 120,000 轮保护）"
    else:
        lbl = "✓ 强 · 符合密码学安全标准（PBKDF2 120,000 轮保护）"
    return valid, lbl, score


def _master_create_or_update_vault(data_dir: str, password: str, existing_vault_key: bytes = None):
    salt = secrets.token_bytes(16)
    k_pwd = hashlib.pbkdf2_hmac("sha256", password.encode("utf-8"), salt, MASTER_PBKDF2_ITERS, dklen=32)
    verifier = hmac.new(k_pwd, b"heyu-master-pwd-verify-v1", hashlib.sha256).digest()
    k_vault = existing_vault_key if (existing_vault_key and len(existing_vault_key) == 32) else secrets.token_bytes(32)
    master_wrapped = _vault_encrypt(k_pwd, k_vault.hex())
    dev_id = _compute_device_id(data_dir)
    k_dev = _compute_device_wrap_key(data_dir)
    dev_wrapped = _vault_encrypt(k_dev, k_vault.hex())
    now_ts = int(time.time())
    meta = {
        "master_pwd_salt": salt.hex(),
        "master_pwd_verifier": verifier.hex(),
        "master_wrapped_vault_key": master_wrapped,
        "device_wrapped_vault_key": dev_wrapped,
        "authorized_device_id": dev_id,
        "last_password_verified_ts": now_ts,
    }
    return meta, k_vault


def _master_verify_password(data_dir: str, password: str, cfg: dict):
    salt_hex = str(cfg.get("master_pwd_salt", "")).strip()
    ver_hex = str(cfg.get("master_pwd_verifier", "")).strip()
    if not salt_hex or not ver_hex or not password:
        return False, None
    try:
        salt = bytes.fromhex(salt_hex)
        expected_ver = bytes.fromhex(ver_hex)
        k_pwd = hashlib.pbkdf2_hmac("sha256", password.encode("utf-8"), salt, MASTER_PBKDF2_ITERS, dklen=32)
        actual_ver = hmac.new(k_pwd, b"heyu-master-pwd-verify-v1", hashlib.sha256).digest()
        if not hmac.compare_digest(actual_ver, expected_ver):
            return False, None
        mw = str(cfg.get("master_wrapped_vault_key", "")).strip()
        if mw:
            dec_hex = _vault_decrypt(k_pwd, mw)
            if dec_hex and len(dec_hex.strip()) == 64:
                return True, bytes.fromhex(dec_hex.strip())
        # Fallback if master_wrapped_vault_key was missing: try device key or mint new
        k_dev = _compute_device_wrap_key(data_dir)
        dw = str(cfg.get("device_wrapped_vault_key", "")).strip()
        if dw:
            dec_hex = _vault_decrypt(k_dev, dw)
            if dec_hex and len(dec_hex.strip()) == 64:
                return True, bytes.fromhex(dec_hex.strip())
        return True, secrets.token_bytes(32)
    except Exception:
        return False, None


def _resolve_authorized_lexicon_key_py(data_dir: str, cfg: dict):
    """
    Returns ('unconfigured', None) | ('new_device', None) | ('ok', k_vault_bytes).
    Matches `src/vault.rs::resolve_authorized_lexicon_key`.
    """
    ver = str(cfg.get("master_pwd_verifier", "")).strip()
    if not ver:
        return "unconfigured", None
    auth_dev = str(cfg.get("authorized_device_id", "")).strip()
    dev_wrapped = str(cfg.get("device_wrapped_vault_key", "")).strip()
    cur_dev = _compute_device_id(data_dir)
    if not auth_dev or not dev_wrapped or auth_dev != cur_dev:
        return "new_device", None
    k_dev = _compute_device_wrap_key(data_dir)
    dec_hex = _vault_decrypt(k_dev, dev_wrapped)
    if not dec_hex or len(dec_hex.strip()) != 64:
        return "new_device", None
    try:
        return "ok", bytes.fromhex(dec_hex.strip())
    except Exception:
        return "new_device", None


def _save_pill_avatar_files(src_path: str) -> bool:
    """Crop image to square, resize to 128x128 RGBA, and write pill_avatar.rgba + pill_avatar.png."""
    try:
        if not src_path or not os.path.isfile(src_path):
            return False
        if os.path.getsize(src_path) > 10 * 1024 * 1024:
            raise ValueError("Avatar image exceeds 10 MB limit")
        from PIL import Image
        Image.MAX_IMAGE_PIXELS = 16_777_216  # 4096x4096 max (decompression bomb defense)
        with Image.open(src_path, formats=("PNG", "JPEG", "WEBP", "BMP")) as raw_img:
            img = raw_img.convert("RGBA")
        w, h = img.size
        side = min(w, h)
        left = (w - side) // 2
        top = (h - side) // 2
        img = img.crop((left, top, left + side, top + side)).resize((128, 128), Image.LANCZOS)
        raw_rgba = struct.pack("<II", 128, 128) + img.tobytes("raw", "RGBA")
        for d in SYNC_DATA_DIRS:
            os.makedirs(d, exist_ok=True)
            with open(os.path.join(d, "pill_avatar.rgba"), "wb") as f:
                f.write(raw_rgba)
            img.save(os.path.join(d, "pill_avatar.png"), "PNG")
        return True
    except Exception as e:
        print(f"[studio] failed to process avatar: {e}", file=sys.stderr)
        return False


def _vault_resolve_key(data_dir: str, pin_salt_hex: str, wrapped_key_hex: str) -> bytes:
    secret = _vault_load_secret(data_dir)
    if pin_salt_hex and wrapped_key_hex:
        try:
            salt = bytes.fromhex(pin_salt_hex)
            wrapped = bytes.fromhex(wrapped_key_hex)
            if len(wrapped) == 32:
                mask = hashlib.sha256(secret + salt).digest()
                return bytes(a ^ b for a, b in zip(wrapped, mask))
        except Exception:
            pass
    return hmac.new(secret, b"heyu-default-history-key-v1", hashlib.sha256).digest()


def _vault_ctr_xor(key: bytes, iv: bytes, data: bytes) -> bytes:
    out = bytearray(data)
    blocks = (len(out) + 31) // 32
    for counter in range(blocks):
        ks = hashlib.sha256(key + iv + struct.pack(">I", counter)).digest()
        start = counter * 32
        end = min(start + 32, len(out))
        for i in range(start, end):
            out[i] ^= ks[i - start]
    return bytes(out)


def _vault_encrypt(key: bytes, plaintext: str) -> str:
    iv = secrets.token_bytes(16)
    ct = _vault_ctr_xor(key, iv, plaintext.encode("utf-8"))
    tag = hmac.new(key, iv + ct, hashlib.sha256).digest()
    return f"HEYUENC1:{iv.hex()}:{tag.hex()}:{ct.hex()}"


def _vault_decrypt(key: bytes, encoded: str):
    s = encoded.strip()
    if not s.startswith("HEYUENC1:"):
        return None
    parts = s[9:].split(":", 2)
    if len(parts) != 3:
        return None
    try:
        iv = bytes.fromhex(parts[0])
        tag = bytes.fromhex(parts[1])
        ct = bytes.fromhex(parts[2])
    except Exception:
        return None
    if len(iv) != 16 or len(tag) != 32:
        return None
    expected_tag = hmac.new(key, iv + ct, hashlib.sha256).digest()
    if not hmac.compare_digest(tag, expected_tag):
        return None
    try:
        return _vault_ctr_xor(key, iv, ct).decode("utf-8")
    except Exception:
        return None


def _vault_secure_wipe(path: str):
    try:
        if os.path.exists(path):
            sz = max(256, min(os.path.getsize(path), 16 * 1024 * 1024))
            with open(path, "r+b") as f:
                f.seek(0)
                f.write(b"\x00" * sz)
                f.flush()
            os.remove(path)
    except Exception:
        try:
            if os.path.exists(path):
                os.remove(path)
        except Exception:
            pass
    legacy = os.path.join(os.path.dirname(path), "input_history.tsv")
    if os.path.exists(legacy):
        try:
            with open(legacy, "wb") as f:
                f.write(b"\x00" * 256)
            os.remove(legacy)
        except Exception:
            pass


def _vault_read_history(data_dir: str, pin_salt_hex: str, wrapped_key_hex: str, retention_mins: int, max_items: int):
    if retention_mins <= 0:
        return []
    enc_path = os.path.join(data_dir, "input_history.enc")
    if not os.path.exists(enc_path):
        return []
    try:
        raw = open(enc_path, "r", encoding="utf-8").read()
    except Exception:
        return []
    key = _vault_resolve_key(data_dir, pin_salt_hex, wrapped_key_hex)
    dec = _vault_decrypt(key, raw)
    if dec is None:
        return []
    cutoff = int(time.time()) - retention_mins * 60
    items = []
    for line in dec.splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t", 2)
        if len(parts) == 3:
            try:
                ts = int(parts[0].strip())
                if ts >= cutoff:
                    items.append((ts, parts[1].strip(), parts[2].replace("\\n", "\n")))
            except Exception:
                pass
    items.sort(key=lambda x: x[0], reverse=True)
    return items[:max(1, max_items)]


def _vault_write_history(data_dirs, pin_salt_hex: str, wrapped_key_hex: str, items):
    if not items:
        for d in data_dirs:
            _vault_secure_wipe(os.path.join(d, "input_history.enc"))
        return
    primary = data_dirs[0] if data_dirs else DATA_DIR
    key = _vault_resolve_key(primary, pin_salt_hex, wrapped_key_hex)
    lines = ["# HEYU Encrypted Input History Vault v1"]
    for ts, src, txt in items:
        esc = txt.replace("\r", "").replace("\n", "\\n").replace("\t", " ")
        lines.append(f"{ts}\t{src}\t{esc}")
    enc = _vault_encrypt(key, "\n".join(lines) + "\n")
    for d in data_dirs:
        try:
            os.makedirs(d, exist_ok=True)
            with open(os.path.join(d, "input_history.enc"), "w", encoding="utf-8") as f:
                f.write(enc)
            legacy = os.path.join(d, "input_history.tsv")
            if os.path.exists(legacy):
                _vault_secure_wipe(legacy)
        except Exception:
            pass


def _vault_verify_pin(pin: str, salt_hex: str, verifier_hex: str) -> bool:
    if not salt_hex or not verifier_hex:
        return True
    try:
        salt = bytes.fromhex(salt_hex)
        expected = bytes.fromhex(verifier_hex)
        dk = hashlib.pbkdf2_hmac("sha256", pin.encode("utf-8"), salt, 10000, dklen=32)
        ver = hmac.new(dk, b"heyu-pin-verifier-v1", hashlib.sha256).digest()
        return hmac.compare_digest(ver, expected)
    except Exception:
        return False


def _vault_create_pin_params(data_dir: str, pin: str):
    salt = secrets.token_bytes(16)
    dk = hashlib.pbkdf2_hmac("sha256", pin.encode("utf-8"), salt, 10000, dklen=32)
    ver = hmac.new(dk, b"heyu-pin-verifier-v1", hashlib.sha256).digest()
    secret = _vault_load_secret(data_dir)
    mask = hashlib.sha256(secret + salt).digest()
    wrapped = bytes(a ^ b for a, b in zip(dk, mask))
    return salt.hex(), ver.hex(), wrapped.hex()


_APPEARANCE_MAP = {"light": "Light", "dark": "Dark", "system": "System"}
_APPEARANCE_LABELS = [("浅色", "light"), ("深色", "dark"), ("跟随系统", "system")]

_ui_prefs = _read_json(STUDIO_UI_PATH, {})
if not isinstance(_ui_prefs, dict):
    _ui_prefs = {}
ctk.set_appearance_mode(_APPEARANCE_MAP.get(_ui_prefs.get("appearance", "system"), "System"))
ctk.set_default_color_theme("blue")

# 26-key atlas: roots and examples come from data/root_atlas.json, generated from the real
# code tables by tools/gen_manual.py (every example is verified against its full code).
_SP_FINALS = {"q": "iu", "w": "ei", "e": "e", "r": "uan", "t": "ue / ve", "y": "un", "u": "u", "i": "i",
              "o": "o / uo", "p": "ie", "a": "a", "s": "ong / iong", "d": "ai", "f": "en", "g": "eng",
              "h": "ang", "j": "an", "k": "ing / uai", "l": "iang / uang", "z": "ou", "x": "ia / ua",
              "c": "ao", "v": "v / ui", "b": "in", "n": "iao", "m": "ian"}
_SP_INIT = {"v": "zh", "i": "ch", "u": "sh"}


def _load_atlas():
    path = os.path.join(DATA_DIR, "root_atlas.json")
    if not os.path.exists(path):
        path = os.path.join(LOCAL_DATA_DIR, "root_atlas.json")
    atlas = _read_json(path, {})
    if not isinstance(atlas, dict):
        atlas = {}
    rows = []
    for k in "abcdefghijklmnopqrstuvwxyz":
        item = atlas.get(k, {})
        rows.append((k.upper(), _SP_INIT.get(k, k), _SP_FINALS[k], " · ".join(item.get("roots", [])),
                     " · ".join(item.get("examples", []))))
    return rows


TRAINING_DRILLS = {
    "1. 一简 26 字极速反射 (1键1字)": "我 的 不 一 这 是 了 你 在 人 他 个 们 就 可以 好 三 从 发 平 去 下 出 二 啊",
    "2. 二简高频词肌肉唤醒 (2键1词)": "我们 什么 知道 时间 没有 因为 所以 已经 可以 这个 自己 怎么 觉得 现在 这样 还是 出来",
    "3. 鹤形双形码拆字找回 (含日月目归O)": "明 早 看 有 国 回 困 鹤 和 河 林 森 休 好 男 情 请 清 晴 进 远 连 起 赶",
    "4. 四码唯一自动上屏盲打冲刺": "鹤羽音形 按部就班 爱搭不理 安定团结 实事求是 无论如何 显而易见 毫无疑问 归根结底",
    "5. 220+ 字/分 幽灵续写与超长句实战": "因为所以 之所以会这样 非常感谢你的帮助 完全没有问题 从某种意义上来说 值得一提的是",
    "6. 中 / 意 / 西 / 英 四语无缝混输": "perché città università buongiorno español mañana año también información",
}

# Read-only shortcut reference: (group, [(title, description, [key combos])])
SHORTCUTS = [
    ("全局", [
        ("切换中文 / 直通英文", "单击左 Shift（预览框有英文时直接上屏并切换）", ["Left Shift"]),
        ("循环切换输入语言", "中文 → Italiano → Español → English", ["Ctrl+Shift+L"]),
        ("快速添加词条", "弹出造词窗口，把选中的文字加入词库", ["Ctrl+Alt+N"]),
        ("语音输入（麦克风连续长对话）", "按下开始说话，停顿即刻切块转写上屏，再按结束", ["Ctrl+Alt+V"]),
        ("系统声音/指定软件内录转写", "直接内录电脑或指定窗口播放的会议、网课或视频声音并转写", ["Ctrl+Alt+B"]),
        ("双光标定点（第二光标位置）", "双光标协同模式下，在任意软件内定点放置独立转录光标", ["Ctrl+Alt+左键", "Ctrl+Alt+G"]),
        ("外语伴学开关", "关闭 → EN → IT → ES → 全部 循环", ["Ctrl+Shift+I"]),
        ("候选框字号", "放大 / 缩小候选框文字", ["Ctrl+Shift+=", "Ctrl+Shift+-"]),
        ("切换候选框主题", "在 9 款主题之间循环", ["Ctrl+Shift+T"]),
        ("盲打模式", "隐藏 / 显示候选框", ["Ctrl+Shift+B"]),
    ]),
    ("候选与编辑", [
        ("麦克风语音输入开关", "中文模式直接输入 oiu 立即开关麦克风；外语模式使用组合快捷键防打字冲突", ["oiu (仅中文)", "Ctrl+Alt+V", "Ctrl+Shift+V"]),
        ("撤销上次打字上屏", "中文模式输入 ocx 整词/整句撤回；外语模式使用组合快捷键", ["ocx (仅中文)", "Ctrl+Shift+Z", "Ctrl+Alt+Z"]),
        ("置顶为首选", "上屏该候选并永久设为此编码的第 1 位", ["Ctrl+1…9"]),
        ("双形唤醒栏选字", "输入两码双拼后，从提示栏直接选字", ["Alt+1…6"]),
        ("整句补全", "接受幽灵续写给出的整句建议", ["Tab"]),
        ("形码反查", "反引号加双拼，显示单字完整四码", ["` + 双拼"]),
        ("内联科学计算器与大写金额", "输入 =加算式实时计算，单引号输出人民币财务大写", ["=125*38"]),
        ("剪贴板快贴 / 加密历史找回", "快速上屏剪贴板内容或最近 9 条本地加密历史", [";v", ";h"]),
        ("重复上次输入", "立即重复上一次上屏的内容", [";f"]),
        ("分号快符", "26 键高频标点一键直出", [";a … ;z"]),
    ]),
    ("中文 o 引导宏 & 全语种正式组合键", [
        ("麦克风语音输入启停", "中文模式输入 oiu 直接启停麦克风；外语模式按 Ctrl+Alt+V / Ctrl+Shift+V", ["oiu", "Ctrl+Alt+V"]),
        ("撤销上次输入", "中文模式输入 ocx 一键撤回上次输入；外语模式按 Ctrl+Shift+Z / Ctrl+Alt+Z", ["ocx", "Ctrl+Shift+Z"]),
        ("日期", "中文模式输入 orq 选日期；外语模式按 Ctrl+Shift+D 直接输出当前语种日期", ["orq", "Ctrl+Shift+D"]),
        ("时间", "中文模式输入 ouj 选时间；外语模式按 Ctrl+Shift+U 直接输出当前时间", ["ouj", "Ctrl+Shift+U"]),
        ("星期", "中文模式输入 oxq 选四语星期；外语模式按 Ctrl+Shift+W 直接输出当前语种星期", ["oxq", "Ctrl+Shift+W"]),
        ("完整时间戳", "中文模式输入 ors 选时间戳；外语模式按 Ctrl+Shift+M 直接输出时间戳", ["ors", "Ctrl+Shift+M"]),
    ]),
]

LICENSES = [
    ("whisper.cpp", "语音识别推理引擎 · Georgi Gerganov 等", "MIT"),
    ("OpenAI Whisper 模型权重", "语音识别模型 · OpenAI", "MIT"),
    ("CustomTkinter", "设置中心界面组件 · Tom Schimansky", "MIT"),
    ("Pillow", "主题预览图解码", "MIT-CMU"),
    ("Python / Tkinter", "设置中心运行时", "PSF"),
    ("Rust 标准库", "输入法引擎 heyu-ime.exe 唯一依赖", "MIT / Apache-2.0"),
]


# =============================================================================
# WIN32 HELPERS (no network APIs — kernel32 / user32 only)
# =============================================================================
class _PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [
        ("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD), ("th32ProcessID", wintypes.DWORD),
        ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", wintypes.DWORD),
        ("cntThreads", wintypes.DWORD), ("th32ParentProcessID", wintypes.DWORD),
        ("pcPriClassBase", ctypes.c_long), ("dwFlags", wintypes.DWORD), ("szExeFile", ctypes.c_wchar * 260),
    ]


def _process_running(name: str) -> bool:
    """Toolhelp snapshot instead of `tasklist` — ~1 ms and never blocks the UI."""
    try:
        k32 = ctypes.windll.kernel32
        k32.CreateToolhelp32Snapshot.restype = ctypes.c_void_p
        snap = k32.CreateToolhelp32Snapshot(0x2, 0)
        if not snap or snap == ctypes.c_void_p(-1).value:
            return False
        try:
            e = _PROCESSENTRY32W()
            e.dwSize = ctypes.sizeof(e)
            ok = k32.Process32FirstW(ctypes.c_void_p(snap), ctypes.byref(e))
            name = name.lower()
            while ok:
                if e.szExeFile.lower() == name:
                    return True
                ok = k32.Process32NextW(ctypes.c_void_p(snap), ctypes.byref(e))
        finally:
            k32.CloseHandle(ctypes.c_void_p(snap))
    except Exception:
        pass
    return False


def _list_audio_target_windows():
    """Enumerate visible top-level windows with their root process PID & exe name for per-app WASAPI loopback."""
    DEFAULT_OPT = "全部系统声音（不限软件）"
    mapping = {DEFAULT_OPT: 0}
    try:
        k32 = ctypes.windll.kernel32
        u32 = ctypes.windll.user32
        k32.CreateToolhelp32Snapshot.restype = ctypes.c_void_p
        snap = k32.CreateToolhelp32Snapshot(0x2, 0)
        pid_exe = {}
        pid_parent = {}
        if snap and snap != ctypes.c_void_p(-1).value:
            try:
                e = _PROCESSENTRY32W()
                e.dwSize = ctypes.sizeof(e)
                ok = k32.Process32FirstW(ctypes.c_void_p(snap), ctypes.byref(e))
                while ok:
                    pid_exe[int(e.th32ProcessID)] = str(e.szExeFile)
                    pid_parent[int(e.th32ProcessID)] = int(e.th32ParentProcessID)
                    ok = k32.Process32NextW(ctypes.c_void_p(snap), ctypes.byref(e))
            finally:
                k32.CloseHandle(ctypes.c_void_p(snap))

        def resolve_root_pid(pid: int) -> int:
            exe = pid_exe.get(pid, "").lower()
            if not exe:
                return pid
            cur = pid
            for _ in range(24):
                ppid = pid_parent.get(cur, 0)
                if not ppid or ppid == cur:
                    break
                if pid_exe.get(ppid, "").lower() == exe:
                    cur = ppid
                else:
                    break
            return cur

        WNDENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
        items = []
        skip_titles = {"Program Manager", "Windows Input Experience", "Microsoft Text Input Application", "设置", WINDOW_TITLE}
        skip_exes = {"heyu-ime.exe", "heyu-voice.exe", "searchhost.exe", "shellexperiencehost.exe", "textinputhost.exe"}
        my_pid = os.getpid()

        def cb(hwnd, _lparam):
            try:
                if not u32.IsWindowVisible(hwnd):
                    return True
                length = u32.GetWindowTextLengthW(hwnd)
                if length <= 0:
                    return True
                buf = ctypes.create_unicode_buffer(length + 2)
                u32.GetWindowTextW(hwnd, buf, length + 1)
                title = buf.value.strip()
                if not title or title in skip_titles:
                    return True
                pid_dw = wintypes.DWORD(0)
                u32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid_dw))
                raw_pid = int(pid_dw.value)
                if raw_pid <= 0 or raw_pid == my_pid:
                    return True
                exe = pid_exe.get(raw_pid, "app.exe")
                if exe.lower() in skip_exes:
                    return True
                root_pid = resolve_root_pid(raw_pid)
                short_exe = exe[:-4] if exe.lower().endswith(".exe") else exe
                short_title = title if len(title) <= 22 else (title[:21] + "…")
                label = f"[{short_exe}] {short_title}"
                items.append((exe.lower(), label, root_pid))
            except Exception:
                pass
            return True

        u32.EnumWindows(WNDENUMPROC(cb), 0)
        items.sort(key=lambda x: (x[0], x[1]))
        for _, label, rpid in items:
            if label not in mapping:
                mapping[label] = rpid
    except Exception:
        pass
    return mapping


MINI_VOICE_TITLE = "鹤羽音形 · 🎙 实时声音转写"
MINI_RECORDER_TITLE = "鹤羽音形 · ◉ 48kHz 录音棚"


def _acquire_single_instance(page, mini_mode=None, prewarm=False):
    """Return a mutex handle if we are the first instance; otherwise forward the request and return None."""
    if os.environ.get("HEYU_STUDIO_MULTI") or mini_mode == "setup":
        return True
    if mini_mode == "voice":
        mutex_name = "Local\\HEYU_MiniVoice_SingleInstance"
        win_title = MINI_VOICE_TITLE
    elif mini_mode == "recorder":
        mutex_name = "Local\\HEYU_MiniRecorder_SingleInstance"
        win_title = MINI_RECORDER_TITLE
    else:
        mutex_name = "Local\\HEYU_Studio_SingleInstance"
        win_title = WINDOW_TITLE
    try:
        k32 = ctypes.windll.kernel32
        k32.CreateMutexW.restype = ctypes.c_void_p
        handle = k32.CreateMutexW(None, False, mutex_name)
        if k32.GetLastError() != 183:  # ERROR_ALREADY_EXISTS
            return handle
    except Exception:
        return True
    if prewarm:
        return None
    if mini_mode is None:
        # Another studio is running: hand over the page and bring its window to the front.
        _write_json(REQUEST_PATH, {"page": page, "ts": time.time()})
    try:
        u32 = ctypes.windll.user32
        u32.FindWindowW.restype = ctypes.c_void_p
        hwnd = u32.FindWindowW(None, win_title)
        if hwnd:
            u32.ShowWindow(ctypes.c_void_p(hwnd), 9)  # SW_RESTORE
            u32.ShowWindow(ctypes.c_void_p(hwnd), 5)  # SW_SHOW
            u32.SetForegroundWindow(ctypes.c_void_p(hwnd))
    except Exception:
        pass
    return None


def _parse_page_arg(argv):
    for i, a in enumerate(argv):
        if a == "--page" and i + 1 < len(argv):
            return argv[i + 1].strip().lower()
        if a.startswith("--page="):
            return a.split("=", 1)[1].strip().lower()
    return None


def _parse_mini_arg(argv):
    for a in argv:
        if a == "--mini-voice":
            return "voice"
        if a == "--mini-recorder":
            return "recorder"
        if a in ("--setup-wizard", "--setup"):
            return "setup"
    return None


def _send_voice_ipc(msg_id: int, cmd_name: str = "") -> bool:
    """Send a control command to `heyu-voice.exe` via `voice_cmd.json` + `WM_TIMER` wakeup, spawning if needed."""
    if not cmd_name:
        cmd_map = {
            0x800A: "toggle_mic",
            0x800B: "toggle_system",
            0x800C: "stop",
            0x800D: "start_mic",
            0x800E: "start_system",
            0x800F: "toggle_both",
            0x8010: "start_both",
        }
        cmd_name = cmd_map.get(msg_id, "")
    if cmd_name:
        seq = int(time.time() * 1000)
        for d in SYNC_DATA_DIRS:
            try:
                _write_json(os.path.join(d, "voice_cmd.json"), {"cmd": cmd_name, "seq": seq})
            except Exception:
                pass
    u32 = ctypes.windll.user32
    u32.FindWindowW.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR]
    u32.FindWindowW.restype = wintypes.HWND
    u32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
    u32.PostMessageW.restype = wintypes.BOOL
    hwnd = u32.FindWindowW("HEYU_VOICE_MSG", None)
    if not hwnd and os.path.exists(VOICE_EXE_PATH):
        try:
            subprocess.Popen([VOICE_EXE_PATH], cwd=os.path.dirname(VOICE_EXE_PATH), creationflags=0x08000000)
            for _ in range(25):
                time.sleep(0.05)
                hwnd = u32.FindWindowW("HEYU_VOICE_MSG", None)
                if hwnd:
                    break
        except Exception:
            pass
    if hwnd:
        # Wake TIMER_CFG (id=2) immediately so `check_config()` processes `voice_cmd.json` in <1ms
        u32.PostMessageW(hwnd, 0x0113, 2, 0)
        return True
    return False


# =============================================================================
# APP
# =============================================================================
class HeyuStudioApp(ctk.CTk):
    NAV = [
        ("general", "常用", "\ue713"),
        ("dict", "词库", "\ue82d"),
        ("voice", "语音输入", "\ue720"),
        ("appearance", "外观", "\ue790"),
        ("security", "安全与隐私", "\uea18"),
        ("shortcuts", "快捷键", "\ue765"),
        ("practice", "练习", "\ue7be"),
        ("about", "关于与许可", "\ue946"),
    ]

    def __init__(self, start_page="general", mini_mode=None):
        super().__init__()
        self.mini_mode = mini_mode
        self._dict_unlocked = False
        self._pin_unlocked = False
        if not _process_running("heyu-ime.exe") and os.path.exists(EXE_PATH):
            try:
                subprocess.Popen(
                    [EXE_PATH],
                    cwd=BASE_DIR,
                    creationflags=subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP,
                )
            except Exception:
                pass

        if mini_mode == "voice":
            self.title(MINI_VOICE_TITLE)
            self.geometry("516x174")
            self.minsize(488, 168)
        elif mini_mode == "recorder":
            self.title(MINI_RECORDER_TITLE)
            self.geometry("492x196")
            self.minsize(468, 190)
        elif mini_mode == "setup":
            self.title("鹤羽音形 · 安全主密码与胶囊身份初始化向导")
            self.geometry("760x720")
            self.minsize(720, 680)
        else:
            self.title(WINDOW_TITLE)
            self.geometry("1180x780")
            self.minsize(1100, 720)
        self.configure(fg_color=C("bg"))
        if os.path.exists(ICON_PATH):
            try:
                self.iconbitmap(ICON_PATH)
            except Exception:
                pass

        self._init_fonts()
        self.cfg = self._load_config()
        self.hud_style = self._load_hud_style()
        self.voice_cfg = self._load_voice_cfg()

        # lazily loaded data
        self.reverse_dict = {}
        self.short_codes = {}
        self._lexicon_state = "idle"     # idle | loading | ready
        self.theme_meta = None
        self._themes_json_ok = False
        self._gallery_images = []
        self._gallery_cards = {}
        self._trees = []
        self._hairlines = []
        self._tree_style = None
        self._sec_report = None

        self.pages = {}
        self._page_built_ms = {}
        self.current_page = None
        self._save_after = {}

        if not _process_running("heyu-ime.exe") and os.path.exists(EXE_PATH):
            try:
                subprocess.Popen(
                    [EXE_PATH],
                    cwd=BASE_DIR,
                    creationflags=subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP,
                )
            except Exception:
                pass

        if mini_mode == "voice":
            self._build_mini_voice_shell()
        elif mini_mode == "recorder":
            self._build_mini_recorder_shell()
        elif mini_mode == "setup":
            self._build_setup_wizard_shell()
        else:
            self._build_shell()
            self.show_page(start_page if start_page in PAGES else "general")
            self._refresh_daemon_status()
            self.after(1500, self._periodic_tick)
            self.after(400, self._poll_requests)
            self.after(280, self._check_security_gates_on_launch)
        ctk.AppearanceModeTracker.add(self._on_mode_changed, self)

    # =========================================================================
    # TYPOGRAPHY / STYLE HELPERS
    # =========================================================================
    def _init_fonts(self):
        try:
            fams = set(tkfont.families(self))
        except Exception:
            fams = set()
        self.ui_family = next((f for f in ("Microsoft YaHei UI", "Microsoft YaHei", "Segoe UI") if f in fams), "Segoe UI")
        self.mono_family = next((f for f in ("Cascadia Mono", "Cascadia Code", "Consolas") if f in fams), "Consolas")
        self.icon_family = next((f for f in ("Segoe Fluent Icons", "Segoe MDL2 Assets") if f in fams), None)
        self._font_cache = {}

    def _ws(self):
        """Effective widget scaling (DPI x user scaling)."""
        try:
            return ctk.ScalingTracker.get_widget_scaling(self)
        except Exception:
            return 1.0

    def F(self, size=14, weight="normal", mono=False, icon=False):
        key = (size, weight, mono, icon)
        if key not in self._font_cache:
            fam = self.icon_family if icon else (self.mono_family if mono else self.ui_family)
            self._font_cache[key] = ctk.CTkFont(family=fam, size=size, weight=weight)
        return self._font_cache[key]

    def _label(self, parent, text, size=14, weight="normal", color="text", mono=False, **kw):
        kw.setdefault("height", 0)
        kw.setdefault("anchor", "w")
        kw.setdefault("justify", "left")
        return ctk.CTkLabel(parent, text=text, font=self.F(size, weight, mono), text_color=C(color), **kw)

    def _card(self, parent, fg="surface", border="border", **kw):
        return ctk.CTkFrame(parent, fg_color=C(fg), corner_radius=RADIUS_CARD, border_width=1,
                            border_color=C(border) if isinstance(border, str) else border, **kw)

    def _btn(self, parent, text, command, kind="secondary", width=0, height=BTN_H, **kw):
        styles = {
            "primary": dict(fg_color=C("accent"), hover_color=C("accent_hover"), text_color=C("on_accent"),
                            border_width=0, font=self.F(13, "bold")),
            "secondary": dict(fg_color=C("surface"), hover_color=C("surface_alt"), text_color=C("text"),
                              border_width=1, border_color=C("border_strong"), font=self.F(13)),
            "danger": dict(fg_color=C("surface"), hover_color=C("danger_soft"), text_color=C("danger"),
                           border_width=1, border_color=C("border_strong"), font=self.F(13)),
            "ghost": dict(fg_color="transparent", hover_color=C("surface_alt"), text_color=C("accent"),
                          border_width=0, font=self.F(13)),
        }
        st = styles[kind]
        st.setdefault("corner_radius", RADIUS_CTRL)
        st.update(kw)
        # width = text width + padding, so a label can never be clipped
        w = max(width, self._text_w(text, st["font"]) + 32)
        return ctk.CTkButton(parent, text=text, command=command, width=w, height=height, **st)

    def _text_w(self, text, font):
        try:
            return int(font.measure(text))
        except Exception:
            return 8 * len(text)

    def _entry(self, parent, width, placeholder="", mono=False):
        return ctk.CTkEntry(
            parent, width=width, height=BTN_H, corner_radius=RADIUS_CTRL, placeholder_text=placeholder,
            font=self.F(14, mono=mono), fg_color=C("surface"), border_color=C("border_strong"), border_width=1,
            text_color=C("text"), placeholder_text_color=C("muted"),
        )

    def _segmented(self, parent, values, command, **kw):
        return ctk.CTkSegmentedButton(
            parent, values=values, command=command, height=30, corner_radius=RADIUS_CTRL, border_width=3,
            font=self.F(13), fg_color=C("seg_bg"), selected_color=C("seg_sel"),
            selected_hover_color=C("seg_sel"), unselected_color=C("seg_bg"),
            unselected_hover_color=C("nav_hover"), text_color=C("text"), **kw,
        )

    def _choice(self, parent, options, current, on_change):
        """Segmented control over (label, key) pairs."""
        labels = [lbl for lbl, _ in options]
        seg = self._segmented(parent, labels, lambda lbl: on_change(next(k for l2, k in options if l2 == lbl)))
        cur = next((lbl for lbl, k in options if k == current), None)
        if cur is not None:
            seg.set(cur)
        return seg

    def _ui_icon(self, name: str, size=(16, 16)):
        """Render crisp 4x-supersampled Pillow vector & flag CTkImage icons (no broken Windows Unicode flags)."""
        cache = getattr(self, "_ui_icon_cache", None)
        if cache is None:
            cache = {}
            self._ui_icon_cache = cache
        key = (name, size)
        if key in cache:
            return cache[key]
        try:
            import math
            from PIL import Image, ImageDraw

            w, h = size
            S = 4
            W, H = w * S, h * S

            def _star_pts(cx, cy, r_out, r_in, rot_deg=-90.0):
                pts = []
                for i in range(10):
                    r = r_out if i % 2 == 0 else r_in
                    ang = math.radians(rot_deg + i * 36.0)
                    pts.append((cx + r * math.cos(ang), cy + r * math.sin(ang)))
                return pts

            def _render(is_dark: bool):
                img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
                d = ImageDraw.Draw(img)
                fg = (232, 234, 238, 255) if is_dark else (34, 38, 46, 255)
                accent = (245, 165, 36, 255) if is_dark else (180, 83, 9, 255)
                white = (255, 255, 255, 255)

                if name.startswith("flag_"):
                    # Rounded flag badge with crisp border
                    mask = Image.new("L", (W, H), 0)
                    md = ImageDraw.Draw(mask)
                    rad = int(2.6 * S)
                    md.rounded_rectangle([1, 1, W - 2, H - 2], radius=rad, fill=255)
                    canvas = Image.new("RGBA", (W, H), (0, 0, 0, 0))
                    cd = ImageDraw.Draw(canvas)

                    if name == "flag_cn":
                        cd.rectangle([0, 0, W, H], fill=(222, 41, 16, 255))
                        cd.polygon(_star_pts(W * 0.25, H * 0.36, H * 0.22, H * 0.088, -90), fill=(255, 222, 0, 255))
                        for sx, sy in ((0.46, 0.16), (0.54, 0.28), (0.54, 0.45), (0.46, 0.58)):
                            cd.polygon(_star_pts(W * sx, H * sy, H * 0.075, H * 0.032, -70), fill=(255, 222, 0, 255))
                    elif name == "flag_gb":
                        cd.rectangle([0, 0, W, H], fill=(1, 33, 105, 255))
                        cd.line([(0, 0), (W, H)], fill=white, width=int(2.6 * S))
                        cd.line([(0, H), (W, 0)], fill=white, width=int(2.6 * S))
                        cd.line([(0, 0), (W, H)], fill=(200, 16, 46, 255), width=int(1.2 * S))
                        cd.line([(0, H), (W, 0)], fill=(200, 16, 46, 255), width=int(1.2 * S))
                        cx, cy = W // 2, H // 2
                        hw, hr = int(2.2 * S), int(1.3 * S)
                        cd.rectangle([cx - hw, 0, cx + hw, H], fill=white)
                        cd.rectangle([0, cy - hw, W, cy + hw], fill=white)
                        cd.rectangle([cx - hr, 0, cx + hr, H], fill=(200, 16, 46, 255))
                        cd.rectangle([0, cy - hr, W, cy + hr], fill=(200, 16, 46, 255))
                    elif name == "flag_it":
                        t1, t2 = W // 3, (W * 2) // 3
                        cd.rectangle([0, 0, t1, H], fill=(0, 146, 70, 255))
                        cd.rectangle([t1, 0, t2, H], fill=(255, 255, 255, 255))
                        cd.rectangle([t2, 0, W, H], fill=(206, 43, 55, 255))
                    elif name == "flag_es":
                        b1, b2 = H // 4, (H * 3) // 4
                        cd.rectangle([0, 0, W, b1], fill=(170, 21, 27, 255))
                        cd.rectangle([0, b1, W, b2], fill=(241, 191, 0, 255))
                        cd.rectangle([0, b2, W, H], fill=(170, 21, 27, 255))
                        # Mini crest emblem
                        ex, ey = int(W * 0.28), H // 2
                        er = int(H * 0.14)
                        cd.rounded_rectangle([ex - er, ey - er, ex + er, ey + er], radius=S, fill=(170, 21, 27, 255))

                    img.paste(canvas, (0, 0), mask)
                    bcol = (255, 255, 255, 55) if is_dark else (0, 0, 0, 45)
                    d.rounded_rectangle([1, 1, W - 2, H - 2], radius=rad, outline=bcol, width=max(2, S // 2))
                elif name in ("globe", "globe_white"):
                    col = white if name.endswith("_white") else ((56, 189, 248, 255) if is_dark else (2, 132, 199, 255))
                    pad = int(1.6 * S)
                    lw = int(1.55 * S)
                    d.ellipse([pad, pad, W - pad, H - pad], outline=col, width=lw)
                    d.ellipse([W * 0.30, pad, W * 0.70, H - pad], outline=col, width=max(2, int(1.25 * S)))
                    d.line([(pad + 2, H * 0.5), (W - pad - 2, H * 0.5)], fill=col, width=max(2, int(1.25 * S)))
                    d.line([(pad + S * 2, H * 0.29), (W - pad - S * 2, H * 0.29)], fill=col, width=max(2, int(1.1 * S)))
                    d.line([(pad + S * 2, H * 0.71), (W - pad - S * 2, H * 0.71)], fill=col, width=max(2, int(1.1 * S)))
                elif name in ("mic", "mic_on", "mic_badge"):
                    col = white if name in ("mic_on", "mic_badge") else fg
                    lw = int(1.7 * S)
                    cx = W * 0.5
                    d.rounded_rectangle([cx - 2.7 * S, 1.5 * S, cx + 2.7 * S, 9.2 * S], radius=int(2.7 * S), fill=col)
                    d.arc([cx - 4.8 * S, 4.0 * S, cx + 4.8 * S, 11.8 * S], start=0, end=180, fill=col, width=lw)
                    d.line([(cx, 11.8 * S), (cx, 14.3 * S)], fill=col, width=lw)
                    d.line([(cx - 3.0 * S, 14.3 * S), (cx + 3.0 * S, 14.3 * S)], fill=col, width=lw)
                elif name in ("speaker", "speaker_on"):
                    col = ((27, 19, 5, 255) if is_dark else white) if name == "speaker_on" else fg
                    lw = int(1.65 * S)
                    pts = [
                        (2.0 * S, 6.0 * S),
                        (4.6 * S, 6.0 * S),
                        (8.2 * S, 2.8 * S),
                        (8.2 * S, 13.2 * S),
                        (4.6 * S, 10.0 * S),
                        (2.0 * S, 10.0 * S),
                    ]
                    d.polygon(pts, fill=col)
                    d.arc([6.5 * S, 4.6 * S, 11.8 * S, 11.4 * S], start=-50, end=50, fill=col, width=lw)
                    d.arc([5.0 * S, 2.4 * S, 14.4 * S, 13.6 * S], start=-52, end=52, fill=col, width=lw)
                elif name in ("headset", "headset_on"):
                    col = white if name == "headset_on" else fg
                    lw = int(1.7 * S)
                    d.arc([2.2 * S, 2.0 * S, W - 2.2 * S, H - 1.5 * S], start=180, end=360, fill=col, width=lw)
                    d.rounded_rectangle([1.6 * S, 7.2 * S, 4.5 * S, 12.6 * S], radius=int(1.4 * S), fill=col)
                    d.rounded_rectangle([W - 4.5 * S, 7.2 * S, W - 1.6 * S, 12.6 * S], radius=int(1.4 * S), fill=col)
                    d.arc([4.5 * S, 8.8 * S, W - 2.4 * S, 14.4 * S], start=0, end=105, fill=col, width=int(1.4 * S))
                elif name == "stop_wave":
                    d.ellipse([1.5 * S, 1.5 * S, W - 1.5 * S, H - 1.5 * S], outline=(255, 255, 255, 190), width=int(1.4 * S))
                    d.rounded_rectangle([5.0 * S, 5.0 * S, W - 5.0 * S, H - 5.0 * S], radius=int(1.3 * S), fill=white)
                elif name == "refresh":
                    lw = int(1.75 * S)
                    pad = int(2.4 * S)
                    d.arc([pad, pad, W - pad, H - pad], start=35, end=315, fill=fg, width=lw)
                    ax, ay = W - pad - int(0.6 * S), int(4.8 * S)
                    d.polygon([(ax, ay - 2.4 * S), (ax + 2.4 * S, ay + 1.2 * S), (ax - 2.4 * S, ay + 1.2 * S)], fill=fg)
                elif name == "target":
                    col = accent
                    lw = int(1.55 * S)
                    pad = int(2.4 * S)
                    d.ellipse([pad, pad, W - pad, H - pad], outline=col, width=lw)
                    r2 = int(5.4 * S)
                    d.ellipse([r2, r2, W - r2, H - r2], fill=col)
                    cx, cy = W // 2, H // 2
                    d.line([(cx, int(0.8 * S)), (cx, int(3.4 * S))], fill=col, width=lw)
                    d.line([(cx, H - int(3.4 * S)), (cx, H - int(0.8 * S))], fill=col, width=lw)
                    d.line([(int(0.8 * S), cy), (int(3.4 * S), cy)], fill=col, width=lw)
                    d.line([(W - int(3.4 * S), cy), (W - int(0.8 * S), cy)], fill=col, width=lw)
                elif name == "lock":
                    col = (52, 211, 153, 255) if is_dark else (4, 120, 87, 255)
                    lw = int(1.7 * S)
                    cx = W * 0.5
                    d.arc([cx - 3.2 * S, 1.8 * S, cx + 3.2 * S, 8.8 * S], start=180, end=360, fill=col, width=lw)
                    d.rounded_rectangle([3.0 * S, 6.8 * S, W - 3.0 * S, 14.2 * S], radius=int(1.8 * S), fill=col)
                    hole = (25, 32, 42, 255) if is_dark else white
                    d.ellipse([cx - 1.1 * S, 9.2 * S, cx + 1.1 * S, 11.4 * S], fill=hole)
                elif name == "hourglass":
                    col = accent
                    lw = int(1.6 * S)
                    pad = int(2.2 * S)
                    d.ellipse([pad, pad + S, W - pad, H - pad + S], outline=col, width=lw)
                    cx, cy = W // 2, (H + S) // 2
                    d.line([(cx - 1.8 * S, int(1.0 * S)), (cx + 1.8 * S, int(1.0 * S))], fill=col, width=lw)
                    d.line([(cx, cy), (cx, cy - 2.8 * S)], fill=col, width=lw)
                    d.line([(cx, cy), (cx + 2.2 * S, cy + 1.4 * S)], fill=col, width=lw)
                elif name == "dual_cursor":
                    c1 = (56, 189, 248, 255) if is_dark else (29, 78, 216, 255)
                    c2 = accent
                    lw = int(1.75 * S)
                    # Left typing I-beam cursor
                    x1 = int(4.4 * S)
                    d.line([(x1, 2.5 * S), (x1, 13.5 * S)], fill=c1, width=lw)
                    d.line([(x1 - 2.0 * S, 2.5 * S), (x1 + 2.0 * S, 2.5 * S)], fill=c1, width=lw)
                    d.line([(x1 - 2.0 * S, 13.5 * S), (x1 + 2.0 * S, 13.5 * S)], fill=c1, width=lw)
                    # Right voice I-beam cursor with acoustic dot
                    x2 = int(11.4 * S)
                    d.line([(x2, 2.5 * S), (x2, 13.5 * S)], fill=c2, width=lw)
                    d.line([(x2 - 2.0 * S, 2.5 * S), (x2 + 2.0 * S, 2.5 * S)], fill=c2, width=lw)
                    d.line([(x2 - 2.0 * S, 13.5 * S), (x2 + 2.0 * S, 13.5 * S)], fill=c2, width=lw)
                    d.ellipse([x2 - 1.6 * S, 6.4 * S, x2 + 1.6 * S, 9.6 * S], fill=c2)
                elif name == "window_app":
                    col = accent
                    lw = int(1.5 * S)
                    d.rounded_rectangle([1.8 * S, 2.4 * S, W - 1.8 * S, H - 2.4 * S], radius=int(2.0 * S), outline=col, width=lw)
                    d.line([(1.8 * S, 5.8 * S), (W - 1.8 * S, 5.8 * S)], fill=col, width=lw)
                elif name == "sun":
                    col = (245, 158, 11, 255)
                    cx, cy = W * 0.5, H * 0.5
                    r = 3.2 * S
                    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=col)
                    lw = int(1.6 * S)
                    for i in range(8):
                        ang = math.radians(i * 45)
                        x0, y0 = cx + 4.6 * S * math.cos(ang), cy + 4.6 * S * math.sin(ang)
                        x1, y1 = cx + 6.6 * S * math.cos(ang), cy + 6.6 * S * math.sin(ang)
                        d.line([(x0, y0), (x1, y1)], fill=col, width=lw)
                elif name == "moon":
                    col = (96, 165, 250, 255) if is_dark else (79, 70, 229, 255)
                    m_img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
                    md = ImageDraw.Draw(m_img)
                    md.ellipse([2.4 * S, 2.2 * S, W - 2.2 * S, H - 2.2 * S], fill=col)
                    md.ellipse([5.8 * S, 0.8 * S, W + 0.6 * S, H - 4.2 * S], fill=(0, 0, 0, 0))
                    img.alpha_composite(m_img)
                elif name in ("rec_dot", "rec_badge"):
                    col = white if name == "rec_badge" else (239, 68, 68, 255)
                    d.ellipse([2.2 * S, 2.2 * S, W - 2.2 * S, H - 2.2 * S], outline=col, width=int(1.6 * S))
                    d.ellipse([5.0 * S, 5.0 * S, W - 5.0 * S, H - 5.0 * S], fill=col)
                elif name == "play":
                    d.polygon([(4.2 * S, 2.6 * S), (13.4 * S, H * 0.5), (4.2 * S, H - 2.6 * S)], fill=fg)
                elif name == "save":
                    lw = int(1.5 * S)
                    d.rounded_rectangle([2.2 * S, 2.2 * S, W - 2.2 * S, H - 2.2 * S], radius=int(1.8 * S), outline=fg, width=lw)
                    d.rectangle([4.8 * S, 2.2 * S, W - 4.8 * S, 6.4 * S], fill=fg)
                    d.rounded_rectangle([4.4 * S, 9.0 * S, W - 4.4 * S, H - 2.2 * S], radius=S, outline=fg, width=lw)
                elif name == "folder_audio":
                    col = accent
                    d.rounded_rectangle([1.8 * S, 4.4 * S, W - 1.8 * S, H - 2.4 * S], radius=int(1.8 * S), fill=col)
                    d.polygon([(1.8 * S, 5.2 * S), (1.8 * S, 2.8 * S), (6.6 * S, 2.8 * S), (8.2 * S, 5.2 * S)], fill=col)
                elif name == "trash":
                    lw = int(1.5 * S)
                    d.line([(2.8 * S, 4.4 * S), (W - 2.8 * S, 4.4 * S)], fill=fg, width=lw)
                    d.rounded_rectangle([5.6 * S, 2.2 * S, W - 5.6 * S, 4.4 * S], radius=S, outline=fg, width=lw)
                    d.rounded_rectangle([3.8 * S, 4.4 * S, W - 3.8 * S, H - 2.2 * S], radius=int(1.5 * S), outline=fg, width=lw)
                    d.line([(6.6 * S, 7.0 * S), (6.6 * S, H - 4.4 * S)], fill=fg, width=lw)
                    d.line([(W - 6.6 * S, 7.0 * S), (W - 6.6 * S, H - 4.4 * S)], fill=fg, width=lw)
                elif name == "scheme_quanpin":
                    # Royal Blue rounded emblem with crisp Pinyin / Flow wave + keyboard keys
                    bg_c = (37, 99, 235, 255) if not is_dark else (59, 130, 246, 255)
                    d.rounded_rectangle([1.0 * S, 1.0 * S, W - 1.0 * S, H - 1.0 * S], radius=int(4.2 * S), fill=bg_c)
                    # Keycaps grid + flowing wave
                    kw = int(W * 0.16)
                    for kx in (0.24, 0.50, 0.76):
                        cx_k = int(W * kx)
                        cy_k = int(H * 0.36)
                        d.rounded_rectangle([cx_k - kw, cy_k - kw, cx_k + kw, cy_k + kw], radius=int(1.2 * S), fill=(255, 255, 255, 235))
                    d.arc([W * 0.18, H * 0.48, W * 0.82, H * 0.84], start=15, end=165, fill=white, width=int(2.0 * S))
                elif name == "scheme_shuangpin":
                    # Emerald Teal rounded emblem with twin lightning / dual-key rhythm
                    bg_c = (5, 150, 105, 255) if not is_dark else (16, 185, 129, 255)
                    d.rounded_rectangle([1.0 * S, 1.0 * S, W - 1.0 * S, H - 1.0 * S], radius=int(4.2 * S), fill=bg_c)
                    bolt = [
                        (W * 0.56, H * 0.16),
                        (W * 0.28, H * 0.54),
                        (W * 0.48, H * 0.54),
                        (W * 0.42, H * 0.84),
                        (W * 0.72, H * 0.46),
                        (W * 0.52, H * 0.46),
                    ]
                    d.polygon(bolt, fill=white)
                elif name == "scheme_yinxing":
                    # Warm Amber-Gold rounded emblem with soaring crane wing & 4-dot seal
                    bg_c = (180, 83, 9, 255) if not is_dark else (245, 158, 11, 255)
                    fg_c = white if not is_dark else (28, 20, 8, 255)
                    d.rounded_rectangle([1.0 * S, 1.0 * S, W - 1.0 * S, H - 1.0 * S], radius=int(4.2 * S), fill=bg_c)
                    wing = [
                        (W * 0.18, H * 0.64),
                        (W * 0.46, H * 0.26),
                        (W * 0.82, H * 0.34),
                        (W * 0.56, H * 0.56),
                        (W * 0.76, H * 0.74),
                    ]
                    d.polygon(wing, fill=fg_c)
                    d.ellipse([W * 0.64, H * 0.18, W * 0.78, H * 0.32], fill=(254, 240, 138, 255) if not is_dark else (255, 255, 255, 255))
                elif name == "pill_capsule":
                    col = accent
                    lw = int(1.65 * S)
                    d.rounded_rectangle([1.4 * S, 3.8 * S, W - 1.4 * S, H - 3.8 * S], radius=int(4.2 * S), outline=col, width=lw)
                    d.ellipse([3.2 * S, 5.4 * S, 7.6 * S, H - 5.4 * S], fill=col)
                    d.ellipse([W - 6.8 * S, 6.2 * S, W - 3.6 * S, H - 6.2 * S], fill=fg)

                return img.resize((w, h), Image.LANCZOS)

            ctk_img = ctk.CTkImage(light_image=_render(False), dark_image=_render(True), size=size)
            cache[key] = ctk_img
            return ctk_img
        except Exception as e:
            print(f"[studio] icon render error ({name}): {e}", file=sys.stderr)
            return None

    def _attach_tooltip(self, widget, text: str):
        """Attach a sleek floating hover tooltip (shows shortcuts & descriptions without crowding buttons)."""
        state = {"after": None, "win": None, "text": text}

        def _hide(_e=None):
            if state["after"] is not None:
                try:
                    self.after_cancel(state["after"])
                except Exception:
                    pass
                state["after"] = None
            w = state["win"]
            state["win"] = None
            if w is not None:
                try:
                    w.destroy()
                except Exception:
                    pass

        def _show():
            state["after"] = None
            if not state["text"]:
                return
            try:
                if not widget.winfo_exists() or not widget.winfo_viewable():
                    return
                is_dark = ctk.get_appearance_mode() == "Dark"
                bg = "#14171f" if is_dark else "#1e222b"
                fg = "#f3f4f6"
                bd = "#3a404d" if is_dark else "#374151"
                tw = tk.Toplevel(self)
                tw.wm_overrideredirect(True)
                try:
                    tw.wm_attributes("-topmost", True)
                except Exception:
                    pass
                tw.configure(bg=bg)
                inner = tk.Frame(tw, bg=bg, padx=10, pady=6, bd=0, highlightthickness=0)
                inner.pack(fill="both", expand=True)
                lbl = tk.Label(
                    inner,
                    text=state["text"],
                    bg=bg,
                    fg=fg,
                    font=(self.ui_family, 9),
                    justify="left",
                    anchor="w",
                )
                lbl.pack()
                tw.update_idletasks()
                req_w = max(40, inner.winfo_reqwidth())
                req_h = max(22, inner.winfo_reqheight())
                self._position_and_round_popup(tw, widget, req_w, req_h, bd, bg, gap=5, small_corner=True)
                state["win"] = tw
            except Exception:
                pass

        def _enter(_e=None):
            _hide()
            state["after"] = self.after(320, _show)

        widget.bind("<Enter>", _enter, add="+")
        widget.bind("<Leave>", _hide, add="+")
        widget.bind("<ButtonPress>", _hide, add="+")
        return state

    def _position_and_round_popup(self, pop_win, anchor_widget, req_w, req_h, border_hex, surface_hex, gap=4, small_corner=False):
        """
        Apply Windows 11 native DWM hardware anti-aliased rounded corners + border to an overrideredirect
        Toplevel AND position it directly below `anchor_widget` using Win32 physical screen rects
        (bypassing CustomTkinter DPI geometry scaling bugs).
        """
        try:
            user32 = ctypes.windll.user32
            dwmapi = ctypes.windll.dwmapi

            class _RECT(ctypes.Structure):
                _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long), ("right", ctypes.c_long), ("bottom", ctypes.c_long)]

            class _MONITORINFO(ctypes.Structure):
                _fields_ = [("cbSize", ctypes.c_ulong), ("rcMonitor", _RECT), ("rcWork", _RECT), ("dwFlags", ctypes.c_ulong)]

            pop_win.update_idletasks()
            raw_id = pop_win.winfo_id()
            hwnd = user32.GetParent(raw_id) or raw_id

            # 1. Native DWM rounded corners (DWMWCP_ROUND=2 or DWMWCP_ROUNDSMALL=3) + DWM 1px border color
            corner_pref = ctypes.c_int(3 if small_corner else 2)
            dwmapi.DwmSetWindowAttribute(hwnd, 33, ctypes.byref(corner_pref), 4)

            bh = str(border_hex).lstrip("#")
            if len(bh) == 6:
                r, g, b = int(bh[0:2], 16), int(bh[2:4], 16), int(bh[4:6], 16)
                colorref = ctypes.c_uint32(r | (g << 8) | (b << 16))
                dwmapi.DwmSetWindowAttribute(hwnd, 34, ctypes.byref(colorref), 4)

            # 2. Exact physical screen rect of anchor_widget via Win32 GetWindowRect
            btn_rc = _RECT()
            btn_hwnd = anchor_widget.winfo_id()
            user32.GetWindowRect(btn_hwnd, ctypes.byref(btn_rc))

            hmon = user32.MonitorFromWindow(btn_hwnd, 2)
            mi = _MONITORINFO()
            mi.cbSize = ctypes.sizeof(_MONITORINFO)
            if hmon and user32.GetMonitorInfoW(hmon, ctypes.byref(mi)):
                wl, wt, wr, wb = mi.rcWork.left, mi.rcWork.top, mi.rcWork.right, mi.rcWork.bottom
            else:
                wl, wt, wr, wb = 0, 0, user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)

            pw = int(req_w)
            ph = int(req_h)
            bx = int(btn_rc.left)
            by = int(btn_rc.bottom + gap)

            if bx + pw + 6 > wr:
                bx = max(wl + 6, min(int(btn_rc.right) - pw, wr - pw - 6))
            if bx < wl + 6:
                bx = wl + 6
            if by + ph + 8 > wb:
                by = max(wt + 6, int(btn_rc.top) - ph - gap)

            pop_win.wm_geometry(f"{pw}x{ph}+{bx}+{by}")
            # HWND_TOPMOST = -1, SWP_NOACTIVATE = 0x0010, SWP_SHOWWINDOW = 0x0040
            user32.SetWindowPos(hwnd, -1, bx, by, pw, ph, 0x0010 | 0x0040)
            pop_win.after(
                10,
                lambda: user32.SetWindowPos(hwnd, -1, bx, by, pw, ph, 0x0010 | 0x0040)
                if pop_win.winfo_exists()
                else None,
            )
            return hwnd, (btn_rc.left, btn_rc.top, btn_rc.right, btn_rc.bottom), (bx, by, bx + pw, by + ph)
        except Exception:
            return None, None, None

    def _icon_dropdown(self, parent, options, current_key, on_change, width=118, height=28, tooltip=None):
        """
        Compact dropdown button with real Pillow CTkImage icons (e.g. flags/globe/cursor/window) on both
        the closed button AND every option in the DWM-rounded popup list.
        options: list of (short_label, key, icon_name, menu_label_or_None)
        """
        state = {"key": current_key, "popup": None, "opts": [], "opened_ts": 0.0}

        def _normalize(opts_in):
            out = []
            for item in opts_in:
                if len(item) == 3:
                    out.append((item[0], item[1], item[2], item[0]))
                else:
                    out.append(item)
            return out

        state["opts"] = _normalize(options)

        def _find_opt(k_or_lbl):
            for sl, k, ic, ml in state["opts"]:
                if k_or_lbl in (k, sl, ml):
                    return sl, k, ic, ml
            return state["opts"][0] if state["opts"] else ("—", "", "globe", "—")

        sl0, k0, ic0, _ = _find_opt(current_key)
        state["key"] = k0
        icon_sz = (17, 12) if str(ic0).startswith("flag_") else (15, 15)

        btn = ctk.CTkButton(
            parent,
            text=f"{sl0} ▾",
            image=self._ui_icon(ic0, icon_sz),
            compound="left",
            width=width,
            height=height,
            corner_radius=8,
            fg_color=C("surface_alt"),
            hover_color=C("nav_hover"),
            border_width=1,
            border_color=C("border_strong"),
            text_color=C("text"),
            font=self.F(12, "bold"),
            anchor="w",
        )

        def _close_popup(_e=None):
            p = state.get("popup")
            state["popup"] = None
            if p is not None:
                try:
                    p.destroy()
                except Exception:
                    pass

        def _set_val(k_or_lbl, notify=False):
            sl, k, ic, _ = _find_opt(k_or_lbl)
            state["key"] = k
            isz = (17, 12) if str(ic).startswith("flag_") else (15, 15)
            btn.configure(text=f"{sl} ▾", image=self._ui_icon(ic, isz))
            if notify and on_change:
                on_change(k)

        def _set_options(new_opts, selected_key=None):
            state["opts"] = _normalize(new_opts)
            target_k = state["key"] if selected_key is None else selected_key
            _set_val(target_k, notify=False)

        def _open_popup():
            if state.get("popup") is not None:
                _close_popup()
                return
            is_dark = ctk.get_appearance_mode() == "Dark"
            P = PALETTES["dark" if is_dark else "light"]
            surf_hex = P["surface"]
            bd_hex = P["border_strong"]

            pop = tk.Toplevel(self)
            state["popup"] = pop
            state["opened_ts"] = time.time()
            pop.wm_overrideredirect(True)
            try:
                pop.wm_attributes("-topmost", True)
            except Exception:
                pass
            pop.configure(bg=surf_hex, bd=0, highlightthickness=0)

            # Inner container has corner_radius=0 because Windows 11 DWM hardware-clips the popup window
            # itself to a smooth anti-aliased rounded rect and draws a 1px hardware border (zero corner bleed!)
            card = ctk.CTkFrame(
                pop,
                fg_color=C("surface"),
                bg_color=surf_hex,
                corner_radius=0,
                border_width=0,
            )
            card.pack(fill="both", expand=True, padx=4, pady=4)

            btn_phys_w = max(width, int(round(btn.winfo_width() / max(0.5, float(self._ws())))))
            row_min_w = max(btn_phys_w + 24, 172)

            for sl, k, ic, ml in state["opts"]:
                sel = (k == state["key"])
                isz = (18, 13) if str(ic).startswith("flag_") else (15, 15)
                row_btn = ctk.CTkButton(
                    card,
                    text=f"{ml}   ✓" if sel else ml,
                    image=self._ui_icon(ic, isz),
                    compound="left",
                    anchor="w",
                    height=28,
                    width=row_min_w,
                    corner_radius=6,
                    bg_color=surf_hex,
                    fg_color=C("accent_soft") if sel else "transparent",
                    hover_color=C("nav_hover"),
                    border_width=0,
                    text_color=C("accent") if sel else C("text"),
                    font=self.F(12, "bold" if sel else "normal"),
                    command=lambda kk=k: (_close_popup(), _set_val(kk, notify=True)),
                )
                row_btn.pack(fill="x", padx=2, pady=1)

            pop.update_idletasks()
            req_w = card.winfo_reqwidth() + 8
            req_h = card.winfo_reqheight() + 8
            _, init_btn_box, pop_box = self._position_and_round_popup(
                pop, btn, req_w, req_h, bd_hex, surf_hex, gap=4, small_corner=False
            )

            # Watch for outside clicks or window movement so the popup never drifts or stays behind
            def _watch():
                if state.get("popup") is not pop:
                    return
                try:
                    if not pop.winfo_exists() or not btn.winfo_exists() or not btn.winfo_viewable():
                        _close_popup()
                        return
                    user32 = ctypes.windll.user32

                    class _RECT(ctypes.Structure):
                        _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long), ("right", ctypes.c_long), ("bottom", ctypes.c_long)]

                    class _POINT(ctypes.Structure):
                        _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]

                    cur_rc = _RECT()
                    user32.GetWindowRect(btn.winfo_id(), ctypes.byref(cur_rc))
                    if init_btn_box and (cur_rc.left != init_btn_box[0] or cur_rc.top != init_btn_box[1]):
                        _close_popup()
                        return

                    if (time.time() - state["opened_ts"]) > 0.18:
                        lmb = user32.GetAsyncKeyState(0x01) & 0x8000
                        rmb = user32.GetAsyncKeyState(0x02) & 0x8000
                        if lmb or rmb:
                            pt = _POINT()
                            user32.GetCursorPos(ctypes.byref(pt))
                            p_rc = _RECT()
                            raw_id = pop.winfo_id()
                            p_hwnd = user32.GetParent(raw_id) or raw_id
                            user32.GetWindowRect(p_hwnd, ctypes.byref(p_rc))
                            in_pop = (p_rc.left <= pt.x <= p_rc.right) and (p_rc.top <= pt.y <= p_rc.bottom)
                            in_btn = (cur_rc.left <= pt.x <= cur_rc.right) and (cur_rc.top <= pt.y <= cur_rc.bottom)
                            if not in_pop and not in_btn:
                                _close_popup()
                                return
                    pop.after(45, _watch)
                except Exception:
                    _close_popup()

            pop.after(45, _watch)
            pop.bind("<Escape>", _close_popup)

        btn.configure(command=_open_popup)
        btn.get = lambda: state["key"]
        btn.set = lambda v: _set_val(v, notify=False)
        btn.set_options = _set_options
        if tooltip:
            self._attach_tooltip(btn, tooltip)
        return btn

    def _icon_choice(self, parent, options, current_key, on_change):
        """
        Segmented button bar with real Pillow CTkImage icons (flags/globe) for settings pages.
        options: list of (label, key, icon_name_or_None)
        """
        bar = ctk.CTkFrame(parent, fg_color=C("seg_bg"), corner_radius=RADIUS_CTRL)
        btns = {}
        state = {"key": current_key}

        def _update_visuals():
            for k, (b, ic) in btns.items():
                sel = (k == state["key"])
                b.configure(
                    fg_color=C("seg_sel") if sel else "transparent",
                    hover_color=C("seg_sel") if sel else C("nav_hover"),
                    text_color=C("text") if sel else C("text_2"),
                    font=self.F(12, "bold" if sel else "normal"),
                )

        def _select(k, notify=True):
            state["key"] = k
            _update_visuals()
            if notify and on_change:
                on_change(k)

        for idx, item in enumerate(options):
            lbl, k = item[0], item[1]
            ic = item[2] if len(item) > 2 else None
            isz = (18, 13) if (ic and str(ic).startswith("flag_")) else (15, 15)
            img = self._ui_icon(ic, isz) if ic else None
            b = ctk.CTkButton(
                bar,
                text=lbl,
                image=img,
                compound="left" if img else "center",
                height=26,
                width=0,
                corner_radius=RADIUS_CTRL - 2,
                border_width=0,
                command=lambda kk=k: _select(kk, notify=True),
            )
            b.pack(side="left", padx=(3 if idx == 0 else 1, 3 if idx == len(options) - 1 else 1), pady=3)
            btns[k] = (b, ic)

        _update_visuals()
        bar.get = lambda: state["key"]
        bar.set = lambda k: _select(k, notify=False)
        return bar

    def _switch(self, parent, value, command):
        holder = {}

        def _sync():
            s = holder["sw"]
            s.configure(border_color=C("accent") if s.get() else C("switch_off"))

        def _cmd():
            _sync()
            command()

        sw = ctk.CTkSwitch(
            parent, text="", width=44, switch_width=44, switch_height=24, command=_cmd,
            progress_color=C("accent"), fg_color=C("switch_off"),
            button_color=("#ffffff", "#f4f5f7"), button_hover_color=("#ffffff", "#ffffff"),
            border_width=3, border_color=C("switch_off"),
        )
        holder["sw"] = sw
        try:  # drop the empty text column so the switch right-aligns exactly
            sw._text_label.grid_remove()
            sw.grid_columnconfigure(1, minsize=0)
        except Exception:
            pass
        sw.select() if value else sw.deselect()
        _sync()
        return sw

    def _chip(self, parent, text, color="text_2", fg="chip", mono=True, size=12):
        return ctk.CTkLabel(parent, text=text, font=self.F(size, mono=mono), text_color=C(color), fg_color=C(fg),
                            corner_radius=6, height=24, width=self._text_w(text, self.F(size, mono=mono)) + 16)

    def _textbox(self, parent, mono=True, size=13, **kw):
        return ctk.CTkTextbox(
            parent, font=self.F(size, mono=mono), fg_color=C("surface_alt"), text_color=C("text"),
            border_width=1, border_color=C("border"), corner_radius=RADIUS_CTRL,
            scrollbar_button_color=C("border_strong"), scrollbar_button_hover_color=C("muted"), **kw,
        )

    # ---- page / group / row scaffolding --------------------------------------
    def _page(self, title, subtitle, scroll=True, max_w=CONTENT_MAX_W):
        """Create a page frame + a centred content column (max width) and the page header."""
        if scroll:
            page = ctk.CTkScrollableFrame(
                self.content, fg_color=C("bg"), corner_radius=0, border_width=0,
                scrollbar_button_color=C("border"), scrollbar_button_hover_color=C("border_strong"),
            )
            try:
                page._parent_canvas.configure(yscrollincrement=12)
            except Exception:
                pass
            col = ctk.CTkFrame(page, fg_color="transparent")
            col.pack(fill="x", padx=PAGE_PX, pady=(0, 32))
            sizer = page._parent_canvas
        else:
            page = ctk.CTkFrame(self.content, fg_color=C("bg"), corner_radius=0)
            col = ctk.CTkFrame(page, fg_color="transparent")
            col.pack(fill="both", expand=True, padx=PAGE_PX, pady=(0, 24))
            sizer = page
        page._col = col
        page._padx = PAGE_PX
        page._scroll = scroll

        def _on_size(event=None, page=page, max_w=max_w):
            try:
                sc = self._ws()
                w = sizer.winfo_width() / sc
                padx = int(max(PAGE_PX, (w - max_w) / 2))
                if abs(padx - page._padx) > 1:
                    page._padx = padx
                    col.pack_configure(padx=padx)
                if page._scroll:
                    self._update_scrollbar(page)
            except Exception:
                pass

        sizer.bind("<Configure>", _on_size, add="+")
        if scroll:
            col.bind("<Configure>", lambda e, p=page: self._update_scrollbar(p), add="+")
        page._on_size = _on_size

        head = ctk.CTkFrame(col, fg_color="transparent")
        head.pack(fill="x", pady=(28, 8))
        self._label(head, title, 24, "bold").pack(anchor="w")
        if subtitle:
            self._label(head, subtitle, 13, color="muted").pack(anchor="w", pady=(4, 0))
        page._head = head
        return page, col

    def _update_scrollbar(self, page):
        """Hide the scrollbar when the page content fits (cleaner than a permanent rail)."""
        try:
            need = page._col.winfo_reqheight() + 40 > page._parent_canvas.winfo_height()
            sb = page._scrollbar
            if need and not sb.winfo_ismapped():
                sb.grid()
            elif not need and sb.winfo_ismapped():
                sb.grid_remove()
                page._parent_canvas.yview_moveto(0)
        except Exception:
            pass

    def _group(self, col, title=None, note=None, pady_top=24):
        if title:
            hdr = ctk.CTkFrame(col, fg_color="transparent")
            hdr.pack(fill="x", pady=(pady_top, 8))
            self._label(hdr, title, 12, "bold", color="muted").pack(side="left", padx=(4, 0))
            if note:
                self._label(hdr, note, 12, color="muted").pack(side="right", padx=(0, 4))
        card = self._card(col)
        card.pack(fill="x", pady=(0 if title else pady_top, 0))
        card._rows = 0
        return card

    def _row(self, card, title, desc=None, stacked=False, wrap=440, badge=None):
        """A setting row: title + description on the left, returns the right-hand control slot."""
        if card._rows:
            self._hairline(card).pack(fill="x", padx=(int(ROW_PX * self._ws()), 0))
        card._rows += 1
        row = ctk.CTkFrame(card, fg_color="transparent")
        row.pack(fill="x", padx=ROW_PX, pady=ROW_PY)
        row.grid_columnconfigure(0, weight=1)
        txt = ctk.CTkFrame(row, fg_color="transparent")
        txt.grid(row=0, column=0, sticky="w")
        if badge:
            line = ctk.CTkFrame(txt, fg_color="transparent")
            line.pack(anchor="w")
            self._label(line, title, 14).pack(side="left")
            ctk.CTkLabel(line, text=badge, font=self.F(11, "bold"), text_color=C("accent"), fg_color=C("accent_soft"),
                         corner_radius=6, height=20, width=self._text_w(badge, self.F(11, "bold")) + 14).pack(side="left", padx=(8, 0))
        else:
            self._label(txt, title, 14).pack(anchor="w")
        row._desc = None
        if desc:
            row._desc = self._label(txt, desc, 12, color="muted", wraplength=wrap if not stacked else 720)
            row._desc.pack(anchor="w", pady=(3, 0))
        slot = ctk.CTkFrame(row, fg_color="transparent")
        if stacked:
            slot.grid(row=1, column=0, columnspan=2, sticky="w", pady=(12, 0))
        else:
            slot.grid(row=0, column=1, sticky="e", padx=(24, 0))
        row.slot = slot
        return row

    def _debounced(self, key, ms, fn):
        if key in self._save_after:
            try:
                self.after_cancel(self._save_after[key])
            except Exception:
                pass
        self._save_after[key] = self.after(ms, fn)

    def _run_bg(self, work, done):
        """Run `work()` in a thread; call `done(result)` on the Tk thread (polled via after)."""
        box = {}

        def _t():
            try:
                box["r"] = work()
            except Exception as e:  # pragma: no cover
                box["e"] = e

        th = threading.Thread(target=_t, daemon=True)
        th.start()

        def _poll():
            if th.is_alive():
                self.after(40, _poll)
            elif "e" in box:
                print(f"[studio] background task failed: {box['e']}", file=sys.stderr)
                done(None)
            else:
                done(box.get("r"))

        self.after(40, _poll)

    # =========================================================================
    # CONFIG I/O
    # =========================================================================
    def _load_config(self) -> dict:
        default_cfg = {
            "theme_index": 0,
            "show_shape_hints": True,
            "auto_commit_4code": True,
            "auto_clear_empty_4code": True,
            "enable_ghost_tab": True,
            "page_size": 10,
            "show_status_pill": True,
            "hide_candidate_hud": False,
            "immersion_mode": "ALL",
            "input_scheme": "yinxing",
            "pill_x": -1,
            "pill_y": -1,
            "default_mode": "ZH",
            "font_scale": 1.15,
            "history_retention_mins": 0,
            "history_max_items": 200,
            "history_pin_salt": "",
            "history_pin_verifier": "",
            "history_wrapped_key": "",
            "user_name": "",
            "avatar_enabled": False,
            "avatar_size": 26,
            "pill_scale": 1.0,
            "pill_items": "badge,cpm,sub,scheme,trad,voice,recorder,quick_add,settings",
            "master_pwd_salt": "",
            "master_pwd_verifier": "",
            "master_wrapped_vault_key": "",
            "device_wrapped_vault_key": "",
            "authorized_device_id": "",
            "last_password_verified_ts": 0,
            "last_lexicon_backup_ts": 0,
        }
        data = _read_json(CONFIG_PATH, None)
        if isinstance(data, dict):
            default_cfg.update(data)
        return default_cfg

    def _save_config(self):
        # Re-read first so keys written by the engine meanwhile (pill position…) are not clobbered.
        disk = _read_json(CONFIG_PATH, None)
        if isinstance(disk, dict):
            for k, v in disk.items():
                if k not in self._cfg_touched:
                    self.cfg[k] = v
        for d in SYNC_DATA_DIRS:
            try:
                os.makedirs(d, exist_ok=True)
                with open(os.path.join(d, "config.json"), "w", encoding="utf-8") as f:
                    json.dump(self.cfg, f, indent=2)
            except Exception:
                pass

    _cfg_touched = set()

    def _set_cfg(self, key, value):
        self.cfg[key] = value
        self._cfg_touched.add(key)
        self._save_config()

    # --- HUD style (data/hud_style.json, read by the Rust HUD) ---------------
    def _load_hud_style(self) -> dict:
        style = {"layout": "horizontal", "glass": True}
        data = _read_json(HUD_STYLE_PATH, None)
        if isinstance(data, dict):
            style.update(data)
        else:
            for d in SYNC_DATA_DIRS:
                _write_json(os.path.join(d, "hud_style.json"), style)
        if style.get("layout") not in ("horizontal", "vertical"):
            style["layout"] = "horizontal"
        style["glass"] = bool(style.get("glass", True))
        return style

    def _save_hud_style(self):
        for d in SYNC_DATA_DIRS:
            _write_json(os.path.join(d, "hud_style.json"), self.hud_style)

    # --- Voice config (data/voice_config.json, read by the voice engine) -------
    def _load_voice_cfg(self) -> dict:
        cfg = dict(VOICE_DEFAULTS)
        for d in [DATA_DIR] + SYNC_DATA_DIRS:
            data = _read_json(os.path.join(d, VOICE_CONFIG_NAME), None)
            if isinstance(data, dict):
                cfg.update(data)
                break
        return cfg

    def _set_voice(self, key, value):
        self.voice_cfg[key] = value
        self._save_voice_cfg()

    def _save_voice_cfg(self):
        for d in SYNC_DATA_DIRS:
            _write_json(os.path.join(d, VOICE_CONFIG_NAME), self.voice_cfg)

    # --- Theme catalogue -------------------------------------------------------
    def _load_theme_meta(self) -> list:
        themes = {t["index"]: dict(t) for t in FALLBACK_THEMES}
        data = _read_json(THEMES_JSON_PATH, None)
        self._themes_json_ok = isinstance(data, list) and len(data) > 0
        if self._themes_json_ok:
            for item in data:
                try:
                    idx = int(item.get("index"))
                except Exception:
                    continue
                merged = themes.get(idx, {"index": idx, "name": f"Theme {idx}", "is_dark": True,
                                          "accent": "#888888", "bg": "#222222"})
                merged.update({k: v for k, v in item.items() if v is not None})
                merged["index"] = idx
                themes[idx] = merged
        return [themes[i] for i in sorted(themes)]

    # --- Lexicon (reverse lookup + short codes), loaded in the background -----
    @staticmethod
    def _parse_lexicon():
        rev, shorts = {}, {}
        if os.path.exists(REVERSE_PATH):
            with open(REVERSE_PATH, "r", encoding="utf-8") as f:
                for line in f:
                    if line.startswith("#") or not line.strip():
                        continue
                    parts = line.strip().split("\t")
                    if len(parts) == 2:
                        rev[parts[0]] = parts[1].split(",")
        if os.path.exists(SYS_DICT_PATH):
            with open(SYS_DICT_PATH, "r", encoding="utf-8") as f:
                for line in f:
                    if line.startswith("#") or not line.strip():
                        continue
                    parts = line.strip().split("\t")
                    if len(parts) >= 4:
                        code, word, rank, layer = parts[0], parts[1], parts[2], parts[3]
                        if code.isalpha():
                            shorts.setdefault(word, []).append((code, layer, rank))
        return rev, shorts

    def _ensure_lexicon(self, then=None):
        if self._lexicon_state == "ready":
            if then:
                then()
            return
        self._lexicon_waiters = getattr(self, "_lexicon_waiters", [])
        if then:
            self._lexicon_waiters.append(then)
        if self._lexicon_state == "loading":
            return
        self._lexicon_state = "loading"

        def _done(res):
            if res:
                self.reverse_dict, self.short_codes = res
            self._lexicon_state = "ready"
            for cb in self._lexicon_waiters:
                try:
                    cb()
                except Exception as e:
                    print(f"[studio] lexicon callback failed: {e}", file=sys.stderr)
            self._lexicon_waiters = []

        self._run_bg(self._parse_lexicon, _done)

    # =========================================================================
    # SHELL: SIDEBAR + CONTENT
    # =========================================================================
    def _build_shell(self):
        self.grid_columnconfigure(1, weight=1)
        self.grid_rowconfigure(0, weight=1)

        side = ctk.CTkFrame(self, width=216, fg_color=C("sidebar"), corner_radius=0)
        side.grid(row=0, column=0, sticky="nsw")
        side.grid_propagate(False)
        side.pack_propagate(False)
        self.sidebar = side

        brand = ctk.CTkFrame(side, fg_color="transparent")
        brand.pack(fill="x", padx=16, pady=(20, 12))
        ctk.CTkLabel(brand, text="鹤", width=38, height=38, corner_radius=11, fg_color=C("accent"),
                     text_color=C("on_accent"), font=self.F(18, "bold")).pack(side="left")
        bt = ctk.CTkFrame(brand, fg_color="transparent")
        bt.pack(side="left", padx=(10, 0))
        self._label(bt, "鹤羽音形", 15, "bold").pack(anchor="w")
        self._label(bt, "全拼 · 双拼 · 音形 · AI语音", 11, color="muted").pack(anchor="w")

        self._nav_items = {}
        nav_sections = {
            "general": "核心输入与外观",
            "voice": "AI 语音与办公",
            "security": "安全与进阶工具",
        }
        for key, label, glyph in self.NAV:
            if key in nav_sections:
                sec_lbl = ctk.CTkLabel(
                    side,
                    text=nav_sections[key],
                    font=self.F(11, "bold"),
                    text_color=C("muted"),
                    anchor="w",
                    height=18,
                )
                sec_lbl.pack(fill="x", padx=18, pady=(10 if key != "general" else 2, 2))
            self._make_nav_item(side, key, label, glyph)

        # bottom: quick Light/Dark appearance switch + engine status
        foot = ctk.CTkFrame(side, fg_color="transparent")
        foot.pack(side="bottom", fill="x", padx=16, pady=(0, 18))
        is_dark_now = ctk.get_appearance_mode() == "Dark"
        self.sb_theme_btn = ctk.CTkButton(
            foot,
            text="🌙 深色模式 · 点击切换浅色" if is_dark_now else "☀ 浅色模式 · 点击切换深色",
            command=self._toggle_sidebar_appearance,
            height=30,
            corner_radius=8,
            fg_color=C("surface"),
            hover_color=C("nav_hover"),
            border_width=1,
            border_color=C("border_strong"),
            text_color=C("text_2"),
            font=self.F(12),
        )
        self.sb_theme_btn.pack(fill="x", pady=(0, 10))
        st = ctk.CTkFrame(foot, fg_color="transparent")
        st.pack(fill="x")
        self.sb_dot = ctk.CTkFrame(st, width=8, height=8, corner_radius=4, fg_color=C("muted"))
        self.sb_dot.pack(side="left", padx=(4, 8))
        self.sb_status = self._label(st, "检测引擎…", 12, color="text_2")
        self.sb_status.pack(side="left")

        self.content = ctk.CTkFrame(self, fg_color=C("bg"), corner_radius=0)
        self.content.grid(row=0, column=1, sticky="nsew")

    def _make_nav_item(self, parent, key, label, glyph):
        item = ctk.CTkFrame(parent, fg_color="transparent", corner_radius=8, height=38)
        item.pack(fill="x", padx=10, pady=1)
        item.pack_propagate(False)
        bar = ctk.CTkFrame(item, width=3, height=16, corner_radius=2, fg_color="transparent")
        bar.pack(side="left", padx=(4, 0))
        if self.icon_family:
            icon = ctk.CTkLabel(item, text=glyph, width=24, height=0, font=self.F(16, icon=True), text_color=C("text_2"))
        else:
            icon = ctk.CTkLabel(item, text="•", width=24, height=0, font=self.F(16), text_color=C("text_2"))
        icon.pack(side="left", padx=(8, 8))
        txt = ctk.CTkLabel(item, text=label, height=0, font=self.F(14), text_color=C("text"), anchor="w")
        txt.pack(side="left", fill="x", expand=True)

        def _enter(_e=None):
            if self.current_page != key:
                item.configure(fg_color=C("nav_hover"))

        def _leave(_e=None):
            if self.current_page != key:
                item.configure(fg_color="transparent")

        for w in (item, bar, icon, txt):
            w.bind("<Button-1>", lambda e, k=key: self.show_page(k))
            w.bind("<Enter>", _enter)
            w.bind("<Leave>", _leave)
            try:
                w.configure(cursor="hand2")
            except Exception:
                pass
        self._nav_items[key] = (item, bar, icon, txt)

    def _style_nav(self):
        for key, (item, bar, icon, txt) in self._nav_items.items():
            sel = key == self.current_page
            item.configure(fg_color=C("nav_sel") if sel else "transparent")
            bar.configure(fg_color=C("accent") if sel else "transparent")
            icon.configure(text_color=C("accent") if sel else C("text_2"))
            txt.configure(font=self.F(14, "bold") if sel else self.F(14))

    def show_page(self, key):
        if key not in PAGES:
            key = "general"
        if key == self.current_page:
            return
        if key not in self.pages:
            t = time.perf_counter()
            builder = getattr(self, f"_build_page_{key}")
            self.pages[key] = builder()
            self._page_built_ms[key] = (time.perf_counter() - t) * 1000
        if self.current_page in self.pages:
            self.pages[self.current_page].pack_forget()
        self.current_page = key
        page = self.pages[key]
        page.pack(fill="both", expand=True)
        self._style_nav()
        hook = getattr(self, f"_on_show_{key}", None)
        if hook:
            hook()

    # =========================================================================
    # PAGE ① 常用 GENERAL
    # =========================================================================
    PILL_ALL_ITEMS = [
        ("badge", "鹤/头像徽章", "左侧显示鹤字/用户名/圆形头像，点击切换中/意/西/英"),
        ("cpm", "实时手速", "显示实时打字速度（如 120 字/分）"),
        ("sub", "状态副标题", "显示编码方案（如 智能全拼）或外语伴学提示"),
        ("scheme", "全/双/音切换", "胶囊显示「全/双/音」按钮，点击一键切换全拼/双拼/音形"),
        ("trad", "简/繁切换", "胶囊显示「简/繁」按钮，一键切换简繁体输出"),
        ("punct", "中/英标点", "胶囊显示「。/.」按钮，一键切换全角/半角标点"),
        ("immersion", "外语伴学", "胶囊显示「伴」按钮，一键切换外语伴学模式"),
        ("blind", "盲打隐藏框", "胶囊显示「盲」按钮，一键隐藏/显示候选框"),
        ("voice", "🎙 语音转写", "打开实时麦克风/电脑声音转写与双光标小窗"),
        ("recorder", "◉ 专业录音棚", "打开 48kHz 高保真应用内录录音棚小窗"),
        ("quick_add", "✏ 快速造词", "一键唤起个人词条快速添加弹窗"),
        ("dict", "📚 加密词库", "一键直达个人加密词库管理页"),
        ("theme", "🎨 主题轮换", "一键在 15 款艺术插画与经典皮肤间循环切换"),
        ("settings", "⚙ 设置中心", "打开鹤羽音形控制中心"),
    ]

    def _build_page_general(self):
        page, col = self._page(
            "常用与方案工作台",
            "清晰分区的输入方案、悬浮胶囊自由组合、候选栏排版与系统设置 · 修改后即刻热重载生效",
        )

        # --- Top Functional Zone Quick-Filter & Shortcut Bar
        zone_bar = ctk.CTkFrame(col, fg_color=C("surface"), corner_radius=10, border_width=1, border_color=C("border"))
        zone_bar.pack(fill="x", pady=(10, 0))
        zb_in = ctk.CTkFrame(zone_bar, fg_color="transparent")
        zb_in.pack(fill="x", padx=12, pady=8)
        self._label(zb_in, "功能分区速达：", 12, "bold", color="muted").pack(side="left", padx=(2, 6))
        self._gen_zone_btns = {}
        for z_key, z_lbl in [
            ("all", "🌟 全部展开"),
            ("scheme", "⌨️ 方案与候选栏"),
            ("pill", "💊 悬浮胶囊定制"),
            ("sys", "🛡️ 隐私与系统"),
        ]:
            b = ctk.CTkButton(
                zb_in,
                text=z_lbl,
                height=28,
                width=0,
                corner_radius=7,
                font=self.F(12, "bold"),
                fg_color=C("accent") if z_key == "all" else C("surface_alt"),
                hover_color=C("accent_hover") if z_key == "all" else C("nav_hover"),
                text_color=C("on_accent") if z_key == "all" else C("text"),
                command=lambda k=z_key: self._filter_general_zone(k),
            )
            b.pack(side="left", padx=3)
            self._gen_zone_btns[z_key] = b

        self._btn(
            zb_in,
            "🎨 15款艺术插画与经典皮肤 ▸",
            lambda: self.show_page("appearance"),
            kind="ghost",
            height=28,
        ).pack(side="right", padx=(4, 0))

        # Compact telemetry strip
        strip = self._card(col)
        strip.pack(fill="x", pady=(12, 0))
        inner = ctk.CTkFrame(strip, fg_color="transparent")
        inner.pack(fill="x", padx=ROW_PX, pady=12)
        inner.grid_columnconfigure((0, 1, 2, 3), weight=1, uniform="tm")
        self._tm = {}
        for i, (k, title, color) in enumerate([("cur", "实时速度", "accent"), ("peak", "历史峰值", "success"),
                                               ("avg", "平均码长", "info"), ("saved", "累计省键", "purple")]):
            cell = ctk.CTkFrame(inner, fg_color="transparent")
            cell.grid(row=0, column=i, sticky="w")
            self._label(cell, title, 12, color="muted").pack(anchor="w")
            v = self._label(cell, "—", 17, "bold", color=color, mono=True)
            v.pack(anchor="w", pady=(2, 0))
            self._tm[k] = v
        self._refresh_telemetry()

        # Containers for clean functional zone filtering
        self._gen_zones = {}
        z_scheme = ctk.CTkFrame(col, fg_color="transparent")
        z_scheme.pack(fill="x")
        z_pill = ctk.CTkFrame(col, fg_color="transparent")
        z_pill.pack(fill="x")
        z_sys = ctk.CTkFrame(col, fg_color="transparent")
        z_sys.pack(fill="x")
        self._gen_zones = {"scheme": z_scheme, "pill": z_pill, "sys": z_sys}

        # =====================================================================
        # ZONE 1: 编码方案与候选栏 (Exquisite 3 Input Scheme Cards + Input Rules)
        # =====================================================================
        g_scheme = self._group(
            z_scheme,
            "一、首选中文输入方案（点击卡片即刻切换 · 胶囊亦可放「全/双/音」一键切）",
            note="支持 40 码整句 Viterbi 解码 · 简拼混输 · 双手 1-9/0 选词",
            pady_top=16,
        )
        scheme_wrap = ctk.CTkFrame(g_scheme, fg_color="transparent")
        scheme_wrap.pack(fill="x", padx=16, pady=(14, 14))
        scheme_wrap.grid_columnconfigure((0, 1, 2), weight=1, uniform="sch")
        self._scheme_cards = {}
        SCHEME_SPECS = [
            (
                "quanpin",
                "scheme_quanpin",
                "智能全拼",
                "120+ 字/分",
                "零学习门槛 · 整句流式输入",
                "⌨ haobuhaoyong → 好不好用",
                "✓ 40 码整句 Viterbi 解码 + 口语习惯词优排\n✓ 超级声母简拼 (wm / jtkh) + 渐进分词选词",
                ("#2563eb", "#60a5fa"),
                ("#eff6ff", "#172554"),
            ),
            (
                "shuangpin",
                "scheme_shuangpin",
                "鹤羽双拼",
                "150+ 字/分",
                "一字两键 · 免背形码整句流式",
                "⌨ hc bu hc ys → 好不好用",
                "✓ 任何声韵母仅需 2 键，击键直接省半\n✓ 支持双拼整句连打 + 候选框鹤形双码助记",
                ("#059669", "#34d399"),
                ("#ecfdf5", "#064e3b"),
            ),
            (
                "yinxing",
                "scheme_yinxing",
                "鹤羽音形",
                "200+ 字/分",
                "四码定长 · 零重码免空格盲打",
                "⌨ hedn → 鹤 (四码唯一直出)",
                "✓ 双拼声韵 + 首末部双形码定长四码\n✓ 四码唯一自动上屏，彻底告别低头选词",
                ("#b45309", "#f5a524"),
                ("#fffbeb", "#2d2112"),
            ),
        ]
        for idx, (s_key, ic_name, s_title, s_speed, s_sub, s_demo, s_desc, acc_pair, tint_pair) in enumerate(SCHEME_SPECS):
            c_frame = ctk.CTkFrame(
                scheme_wrap,
                fg_color=C("surface"),
                corner_radius=12,
                border_width=2,
                border_color=C("border"),
            )
            c_frame.grid(row=0, column=idx, sticky="nsew", padx=(0 if idx == 0 else 6, 0 if idx == 2 else 6))

            # Top Header Row: 4x Pillow Emblem Icon + Title/Sub + Speed Badge
            top_row = ctk.CTkFrame(c_frame, fg_color="transparent")
            top_row.pack(fill="x", padx=14, pady=(14, 6))
            ic_lbl = ctk.CTkLabel(top_row, text="", image=self._ui_icon(ic_name, (36, 36)), width=36, height=36)
            ic_lbl.pack(side="left", padx=(0, 10))

            title_col = ctk.CTkFrame(top_row, fg_color="transparent")
            title_col.pack(side="left", fill="x", expand=True)
            head_line = ctk.CTkFrame(title_col, fg_color="transparent")
            head_line.pack(fill="x")
            t_lbl = self._label(head_line, s_title, 15, "bold", color="text")
            t_lbl.pack(side="left")
            spd_badge = ctk.CTkLabel(
                head_line,
                text=s_speed,
                font=self.F(11, "bold", mono=True),
                fg_color=tint_pair,
                text_color=acc_pair,
                corner_radius=6,
                height=20,
                padx=7,
            )
            spd_badge.pack(side="right")
            sub_lbl = self._label(title_col, s_sub, 12, color="text_2")
            sub_lbl.pack(anchor="w", pady=(2, 0))

            # Keystroke Visual Demo Pill
            demo_box = ctk.CTkFrame(c_frame, fg_color=C("surface_sunken"), corner_radius=7)
            demo_box.pack(fill="x", padx=14, pady=(4, 8))
            demo_lbl = ctk.CTkLabel(
                demo_box,
                text=s_demo,
                font=self.F(11, "bold", mono=True),
                text_color=acc_pair,
                anchor="w",
                height=24,
            )
            demo_lbl.pack(fill="x", padx=10, pady=2)

            # Concise 2-line feature highlights
            d_lbl = self._label(c_frame, s_desc, 11, color="text_2", justify="left", wraplength=225)
            d_lbl.pack(anchor="w", padx=14, pady=(0, 10))

            # Sleek Selection Button
            act_btn = ctk.CTkButton(
                c_frame,
                text="启用此方案",
                height=30,
                corner_radius=8,
                font=self.F(12, "bold"),
                fg_color=C("surface_alt"),
                hover_color=C("nav_hover"),
                border_width=1,
                border_color=C("border_strong"),
                text_color=C("text"),
                command=lambda k=s_key: self._select_input_scheme(k),
            )
            act_btn.pack(fill="x", padx=14, pady=(0, 14))

            for w in (c_frame, top_row, ic_lbl, title_col, head_line, t_lbl, spd_badge, sub_lbl, demo_box, demo_lbl, d_lbl):
                targets = [w, getattr(w, "_canvas", None), getattr(w, "_label", None)]
                for tgt in targets:
                    if tgt is not None:
                        try:
                            tgt.bind("<Button-1>", lambda e, k=s_key: self._select_input_scheme(k), add="+")
                            tgt.configure(cursor="hand2")
                        except Exception:
                            pass
            self._scheme_cards[s_key] = (c_frame, t_lbl, sub_lbl, act_btn, acc_pair, tint_pair)

        # --- Input & Candidate Bar Settings
        g = self._group(z_scheme, "二、候选词数量、输入习惯与标点符号")
        r = self._row(
            g,
            "每页候选词数量",
            "自定义候选栏单页显示的词条个数（5～10 个）· 选 10 个时可用双手数字键 1～9 及 0 键直选第 10 词",
            stacked=True,
            badge="双手十指 1-9, 0",
        )
        cur_ps = str(max(5, min(10, int(self.cfg.get("page_size", 10 if self.cfg.get("input_scheme") == "quanpin" else 5)))))
        self.page_size_seg = self._choice(
            r.slot,
            [("5 个", "5"), ("6 个", "6"), ("7 个", "7"), ("8 个", "8"), ("9 个", "9"), ("10 个 (1-9, 0 双手)", "10")],
            cur_ps,
            lambda k: self._on_page_size_change(int(k)),
        )
        self.page_size_seg.pack(anchor="w")
        r = self._row(g, "默认输入语言模式", "引擎启动时使用的语言 · Ctrl+Shift+L 循环切换 · 单击左 Shift 直通英文", stacked=True)
        self._choice(r.slot, [("中文", "ZH"), ("Italiano", "IT"), ("Español", "ES"), ("English", "EN"), ("直通英文", "DIRECT")],
                     self.cfg.get("default_mode", "ZH"), lambda k: self._set_cfg("default_mode", k)).pack(anchor="w")
        r = self._row(g, "候选框文字大小", "调整打字候选栏字号 · Ctrl+Shift+= / Ctrl+Shift+- 也可调整")
        box = ctk.CTkFrame(r.slot, fg_color="transparent")
        box.pack()
        cur = float(self.cfg.get("font_scale", 1.15))
        self.lbl_font_scale = self._label(box, f"{int(round(cur * 100))}%", 13, "bold", mono=True, width=52, anchor="e")
        self.font_slider = ctk.CTkSlider(box, from_=0.75, to=2.20, number_of_steps=29, width=220, height=18,
                                         progress_color=C("accent"), button_color=C("accent"),
                                         button_hover_color=C("accent_hover"), fg_color=C("switch_off"),
                                         command=self._on_font_slider)
        self.font_slider.set(cur)
        self.font_slider.pack(side="left")
        self.lbl_font_scale.pack(side="left", padx=(12, 0))
        r = self._row(g, "外语伴学", "打中文时同步显示对应外语单词 · Ctrl+Shift+I 切换")
        self.imm_seg = self._choice(r.slot, [("关闭", "OFF"), ("EN", "EN"), ("IT", "IT"), ("ES", "ES"), ("全部", "ALL")],
                                    str(self.cfg.get("immersion_mode", "ALL")).upper(),
                                    lambda k: self._set_cfg("immersion_mode", k))
        self.imm_seg.pack()
        r = self._row(
            g,
            "语种原生标点符号自动适配",
            "中文模式自动输出全角标点【】、（）、“”、‘’、；、￥、～、……、——；西语/意语自动配对 ¿?、¡!、«»、€",
            badge="多语种标点",
        )
        self._cfg_switch(r.slot, "chinese_punctuation").pack()
        r = self._row(g, "繁体中文输出", "开启后将中文候选词与形码提示实时转换为繁体字 · 也可在悬浮胶囊放「简/繁」按钮一键切换")
        self._cfg_switch(r.slot, "traditional_chinese").pack()
        r = self._row(g, "四码唯一自动上屏", "四码只有一个候选时直接上屏，无需按空格（仅在鹤羽音形模式下对四码唯一词条生效）")
        self._cfg_switch(r.slot, "auto_commit_4code").pack()
        r = self._row(g, "空码自动清屏", "输满四码没有匹配时自动清空，不卡手")
        self._cfg_switch(r.slot, "auto_clear_empty_4code").pack()
        r = self._row(g, "鹤形记忆提示", "输入两码双拼时，在候选词旁与下方提示栏显示各字的双形码")
        self._cfg_switch(r.slot, "show_shape_hints").pack()
        r = self._row(g, "幽灵续写与 Tab 整句补全", "高亮显示 [Tab ⇥] 预测续写词句，按 Tab 键一键上屏 · 输入 =算式 实时计算")
        self._cfg_switch(r.slot, "enable_ghost_tab").pack()
        r = self._row(g, "盲打模式 (隐藏候选框)", "隐藏候选框，适合已熟练的肌肉记忆盲打 · Ctrl+Shift+B")
        self._cfg_switch(r.slot, "hide_candidate_hud").pack()

        # =====================================================================
        # ZONE 2: 悬浮状态胶囊 · 整体尺寸、文字大小与功能自由组合 (Modular Pill Composer)
        # =====================================================================
        g_pill = self._group(
            z_pill,
            "三、💊 悬浮状态胶囊 · 整体大小、文字字号与功能自由组合",
            note="支持极简模式（仅留鹤+设置）、自由增减任意功能按钮或一键隐藏",
            pady_top=18,
        )
        r_vis = self._row(
            g_pill,
            "显示桌面悬浮状态胶囊",
            "关闭后彻底隐藏桌面胶囊（仍可通过任务栏右下角托盘图标或快捷键打开所有功能）",
            badge="显示/隐藏",
        )
        self.sw_show_pill = self._cfg_switch(r_vis.slot, "show_status_pill")
        self.sw_show_pill.pack()

        # Overall Capsule & Text Scale Control (`pill_scale`)
        ps_val = max(0.70, min(1.65, float(self.cfg.get("pill_scale", 1.0))))
        r_psc = self._row(
            g_pill,
            "胶囊整体与文字大小调整",
            "同步缩放悬浮胶囊整体尺寸、内部文字大小（字/分、状态副标题）与按钮图标大小（70% ～ 160%）",
            stacked=True,
            badge="整体与文字缩放",
        )
        psc_wrap = ctk.CTkFrame(r_psc.slot, fg_color="transparent")
        psc_wrap.pack(fill="x")
        for p_lbl, p_v in [("小巧 80%", 0.80), ("标准 100%", 1.00), ("舒适 115%", 1.15), ("大字 130%", 1.30), ("特大 150%", 1.50)]:
            self._btn(psc_wrap, p_lbl, lambda v=p_v: self._apply_pill_scale(v), height=28).pack(side="left", padx=(0, 6))
        self.pill_scale_slider = ctk.CTkSlider(
            psc_wrap,
            from_=0.70,
            to=1.60,
            number_of_steps=18,
            width=200,
            height=18,
            fg_color=C("seg_bg"),
            progress_color=C("accent"),
            button_color=C("accent"),
            button_hover_color=C("accent_hover"),
            command=lambda v: self._apply_pill_scale(float(v), from_slider=True),
        )
        self.pill_scale_slider.set(ps_val)
        self.pill_scale_slider.pack(side="left", padx=(8, 8))
        self.lbl_pill_scale = self._label(psc_wrap, f"{int(round(ps_val * 100))}%", 14, "bold", color="accent", mono=True, width=54)
        self.lbl_pill_scale.pack(side="left")

        # 1-Click Presets for Capsule Composition
        r_pre = self._row(
            g_pill,
            "胶囊布局一键预设",
            "一键切换「极简模式（仅留鹤+设置）」或包含编码切换、词库、主题等丰富组合",
            stacked=True,
            badge="一键切换",
        )
        pre_row = ctk.CTkFrame(r_pre.slot, fg_color="transparent")
        pre_row.pack(fill="x")
        PILL_PRESETS = [
            ("✨ 极简模式 (仅鹤+设置)", "badge,settings", True),
            ("⚡ 精简方案版 (鹤+全/双/音+语音+设置)", "badge,scheme,voice,settings", True),
            ("🛠 标准推荐", "badge,cpm,sub,scheme,trad,voice,recorder,quick_add,settings", True),
            ("🚀 全功能满配 (14项)", "badge,cpm,sub,scheme,trad,punct,immersion,blind,voice,recorder,quick_add,dict,theme,settings", True),
            ("🙈 隐藏胶囊", None, False),
        ]
        for b_txt, items_str, vis_flag in PILL_PRESETS:
            self._btn(
                pre_row,
                b_txt,
                lambda it=items_str, vf=vis_flag: self._apply_pill_preset(it, vf),
                kind="primary" if b_txt.startswith("✨") else "secondary",
                height=30,
            ).pack(side="left", padx=(0, 6), pady=2)

        # Interactive 14-Module Free Composition Grid
        r_mod = self._row(
            g_pill,
            "胶囊显示内容与功能按钮自由组合（点击卡片即可添加/移除）",
            "自由勾选希望放在悬浮胶囊上的显示项与快捷功能，胶囊宽度会根据所选项目自动伸缩精简：",
            stacked=True,
            badge="14 项自由组合",
        )
        mod_wrap = ctk.CTkFrame(r_mod.slot, fg_color="transparent")
        mod_wrap.pack(fill="x")
        self.pill_live_preview_lbl = ctk.CTkLabel(
            mod_wrap,
            text="",
            font=self.F(12, "bold", mono=True),
            fg_color=C("surface_sunken"),
            text_color=C("accent"),
            corner_radius=8,
            height=32,
            anchor="w",
            padx=12,
        )
        self.pill_live_preview_lbl.pack(fill="x", pady=(0, 10))

        grid_f = ctk.CTkFrame(mod_wrap, fg_color="transparent")
        grid_f.pack(fill="x")
        grid_f.grid_columnconfigure((0, 1, 2), weight=1, uniform="pmod")
        self._pill_item_btns = {}
        for idx, (ikey, ilbl, idesc) in enumerate(self.PILL_ALL_ITEMS):
            rr, cc = divmod(idx, 3)
            btn = ctk.CTkButton(
                grid_f,
                text=f"✓ {ilbl}",
                height=34,
                corner_radius=8,
                anchor="w",
                font=self.F(12, "bold"),
                fg_color=C("surface_alt"),
                hover_color=C("nav_hover"),
                border_width=1,
                border_color=C("border"),
                text_color=C("text"),
                command=lambda k=ikey: self._toggle_pill_item(k),
            )
            btn.grid(row=rr, column=cc, sticky="nsew", padx=(0 if cc == 0 else 4, 0 if cc == 2 else 4), pady=4)
            self._attach_tooltip(btn, f"{ilbl}：{idesc}")
            self._pill_item_btns[ikey] = (btn, ilbl)

        # --- Status Pill Identity (Username / Custom Avatar / Avatar Size)
        g_id = self._group(z_pill, "四、悬浮胶囊个性化身份（用户名 / 自定义圆形头像）", note="自定义用户名或上传圆形头像取代默认「鹤」字")
        r = self._row(g_id, "胶囊显示用户名", "留空则默认显示「鹤」字；输入用户名后将直接显示在悬浮状态胶囊左侧徽章上")
        id_box = ctk.CTkFrame(r.slot, fg_color="transparent")
        id_box.pack()
        self.gen_uname_entry = self._entry(id_box, 160, "留空默认显示「鹤」")
        cur_uname = str(self.cfg.get("user_name", "")).strip()
        if cur_uname:
            self.gen_uname_entry.insert(0, cur_uname)
        self.gen_uname_entry.pack(side="left", padx=(0, 8))
        self.gen_uname_entry.bind("<Return>", lambda e: self._save_pill_identity_from_ui())
        self._btn(id_box, "保存名称", self._save_pill_identity_from_ui, kind="primary", height=30).pack(side="left")

        r = self._row(g_id, "胶囊自定义头像", "上传图片取代胶囊上的「鹤」字（自动居中裁剪为高清抗锯齿圆形徽章）")
        av_box = ctk.CTkFrame(r.slot, fg_color="transparent")
        av_box.pack()
        av_on = bool(self.cfg.get("avatar_enabled", False))
        self.gen_av_status_lbl = self._label(
            av_box,
            "✓ 已启用头像" if av_on else "当前显示文字徽章",
            12,
            "bold",
            color="success" if av_on else "muted",
        )
        self.gen_av_status_lbl.pack(side="left", padx=(0, 10))
        self._btn(av_box, "🖼 上传头像…", self._upload_pill_avatar_dialog, kind="primary", height=30).pack(side="left", padx=(0, 6))
        self._btn(av_box, "恢复文字「鹤」", self._clear_pill_avatar, height=30).pack(side="left")

        r = self._row(g_id, "胶囊头像显示大小", "实时调整头像在状态胶囊上的像素尺寸（16px ～ 36px，默认 26px）")
        av_sz = max(16, min(36, int(self.cfg.get("avatar_size", 26))))
        self.lbl_gen_av_sz = self._label(r.slot, f"{av_sz} px", 13, "bold", color="accent", mono=True, width=54)
        self.gen_av_slider = ctk.CTkSlider(
            r.slot, from_=16, to=36, number_of_steps=20, width=180, height=18,
            fg_color=C("seg_bg"), progress_color=C("accent"), button_color=C("accent"),
            button_hover_color=C("accent_hover"), command=self._on_avatar_size_slider,
        )
        self.gen_av_slider.set(av_sz)
        self.gen_av_slider.pack(side="left")
        self.lbl_gen_av_sz.pack(side="left", padx=(10, 0))

        # =====================================================================
        # ZONE 3: 安全与系统
        # =====================================================================
        g_sec_banner = self._group(z_sys, "五、安全与数据隐私保护（第一优先级）", note="顶级密文加密 · 默认用完即毁零残留")
        ret_mins = int(self.cfg.get("history_retention_mins", 0))
        sec_desc = (
            f"• 词库加密：PBKDF2-HMAC-SHA256 (120,000 轮) + 256-bit 认证流加密（{'✓ 主密码已启用' if self._has_master_password() else '⚠ 待设置主密码'}）\n"
            f"• 打字历史：{'✓ 关闭 · 用完即毁（默认零残留）' if ret_mins <= 0 else f'加密暂存 {ret_mins} 分钟后全零覆写销毁'}\n"
            "• 语音数据：✓ 除非手动点击录音棚录音，否则实时转写音频 100% 纯内存处理、用完即毁"
        )
        r = self._row(g_sec_banner, "当前安全防护状态", sec_desc, wrap=540, badge="最高优先级")
        self._btn(r.slot, "🛡️ 打开「安全与隐私」专栏", lambda: self.show_page("security"), kind="primary").pack()

        g_sys = self._group(z_sys, "六、系统与服务")
        mode_tag = "当前：绿色便携版（portable.flag 已启用，数据完全保存在本目录）" if IS_PORTABLE else "登录 Windows 时自动在后台启动鹤羽音形引擎与语音服务（免管理员权限）"
        r = self._row(g_sys, "开机自动启动", mode_tag, badge="免管理员")
        self.sw_autostart = self._switch(r.slot, self._is_autostart_enabled(), self._toggle_autostart)
        self.sw_autostart.pack()
        r = self._row(g_sys, "输入法引擎", "修改词库或升级后可重启引擎")
        self._btn(r.slot, "重启引擎", self.restart_daemon).pack()

        self._refresh_scheme_cards_ui()
        self._refresh_pill_composer_ui()
        return page

    def _filter_general_zone(self, zone_key: str):
        for k, b in getattr(self, "_gen_zone_btns", {}).items():
            sel = (k == zone_key)
            b.configure(
                fg_color=C("accent") if sel else C("surface_alt"),
                hover_color=C("accent_hover") if sel else C("nav_hover"),
                text_color=C("on_accent") if sel else C("text"),
            )
        for k, frame in getattr(self, "_gen_zones", {}).items():
            if zone_key == "all" or k == zone_key:
                if not frame.winfo_ismapped():
                    frame.pack(fill="x")
            else:
                if frame.winfo_ismapped():
                    frame.pack_forget()

    def _get_active_pill_items(self) -> list:
        raw = str(self.cfg.get("pill_items", "badge,cpm,sub,scheme,trad,voice,recorder,quick_add,settings"))
        valid_keys = [k for k, _, _ in self.PILL_ALL_ITEMS]
        out = []
        for tok in raw.split(","):
            t = tok.strip().lower()
            if t in valid_keys and t not in out:
                out.append(t)
        if not out:
            out = ["badge", "settings"]
        return out

    def _set_active_pill_items(self, items_list: list):
        valid_keys = [k for k, _, _ in self.PILL_ALL_ITEMS]
        ordered = [k for k in valid_keys if k in items_list]
        if not ordered:
            ordered = ["badge", "settings"]
        self._set_cfg("pill_items", ",".join(ordered))
        self._refresh_pill_composer_ui()

    def _toggle_pill_item(self, item_key: str):
        cur = self._get_active_pill_items()
        if item_key in cur:
            if len(cur) > 1:
                cur.remove(item_key)
        else:
            cur.append(item_key)
        if not self.cfg.get("show_status_pill", True):
            self._set_cfg("show_status_pill", True)
            if hasattr(self, "sw_show_pill"):
                self.sw_show_pill.select()
        self._set_active_pill_items(cur)

    def _apply_pill_preset(self, items_str, visible: bool):
        self._set_cfg("show_status_pill", bool(visible))
        if hasattr(self, "sw_show_pill"):
            if visible:
                self.sw_show_pill.select()
            else:
                self.sw_show_pill.deselect()
        if items_str:
            self._set_cfg("pill_items", items_str)
        self._refresh_pill_composer_ui()

    def _apply_pill_scale(self, val: float, from_slider: bool = False):
        v = max(0.70, min(1.65, round(float(val), 2)))
        if hasattr(self, "lbl_pill_scale"):
            self.lbl_pill_scale.configure(text=f"{int(round(v * 100))}%")
        if not from_slider and hasattr(self, "pill_scale_slider"):
            self.pill_scale_slider.set(v)
        self._debounced("pill_scale", 80, lambda: (self._set_cfg("pill_scale", v), self._refresh_pill_composer_ui()))

    def _refresh_pill_composer_ui(self):
        active = self._get_active_pill_items()
        for ikey, (btn, ilbl) in getattr(self, "_pill_item_btns", {}).items():
            sel = ikey in active
            btn.configure(
                text=f"✓ {ilbl}" if sel else f"○ {ilbl}",
                fg_color=C("accent_soft") if sel else C("surface_alt"),
                border_color=C("accent") if sel else C("border"),
                text_color=C("accent") if sel else C("text_2"),
            )
        if hasattr(self, "pill_live_preview_lbl"):
            vis = bool(self.cfg.get("show_status_pill", True))
            sc = float(self.cfg.get("pill_scale", 1.0))
            if not vis:
                self.pill_live_preview_lbl.configure(
                    text="🙈 当前状态胶囊已隐藏（可随时点击上方预设或开关恢复显示）",
                    text_color=C("muted"),
                )
            else:
                token_preview = []
                if "badge" in active:
                    un = str(self.cfg.get("user_name", "")).strip()
                    token_preview.append(f"[{un[:4] if un else '鹤'}]")
                if "cpm" in active:
                    token_preview.append("120 字/分")
                if "sub" in active:
                    sc_map = {"quanpin": "智能全拼", "shuangpin": "鹤羽双拼", "yinxing": "鹤羽音形"}
                    token_preview.append(f"· {sc_map.get(self.cfg.get('input_scheme', 'yinxing'), '鹤羽音形')}")
                btn_icons = {
                    "scheme": "[全/双/音]",
                    "trad": "[简/繁]",
                    "punct": "[。/.]",
                    "immersion": "[伴]",
                    "blind": "[盲]",
                    "voice": "[🎙]",
                    "recorder": "[◉]",
                    "quick_add": "[✏]",
                    "dict": "[词]",
                    "theme": "[彩]",
                    "settings": "[⚙]",
                }
                btns_str = " ".join(btn_icons[k] for k in active if k in btn_icons)
                if btns_str and token_preview:
                    preview_s = " ".join(token_preview) + "  ｜  " + btns_str
                else:
                    preview_s = " ".join(token_preview) + btns_str
                self.pill_live_preview_lbl.configure(
                    text=f"💊 实时胶囊预览 ({int(round(sc * 100))}%)：  {preview_s}",
                    text_color=C("accent"),
                )

    def _on_show_general(self):
        self._refresh_telemetry()
        self._refresh_scheme_cards_ui()
        self._refresh_pill_composer_ui()
        if hasattr(self, "gen_uname_entry"):
            cur = str(self.cfg.get("user_name", "")).strip()
            if self.gen_uname_entry.get().strip() != cur:
                self.gen_uname_entry.delete(0, "end")
                if cur:
                    self.gen_uname_entry.insert(0, cur)
        if hasattr(self, "gen_av_status_lbl"):
            av_on = bool(self.cfg.get("avatar_enabled", False))
            self.gen_av_status_lbl.configure(
                text="✓ 已启用头像" if av_on else "当前显示文字徽章",
                text_color=C("success") if av_on else C("muted"),
            )

    def _on_page_size_change(self, count: int):
        count = max(5, min(10, int(count)))
        self.cfg["page_size_custom"] = True
        self._set_cfg("page_size", count)

    def _select_input_scheme(self, scheme_key: str):
        if scheme_key not in ("quanpin", "shuangpin", "yinxing"):
            scheme_key = "yinxing"
        if scheme_key == "quanpin" and not self.cfg.get("page_size_custom"):
            self._set_cfg("page_size", 10)
        self._set_cfg("input_scheme", scheme_key)
        self._refresh_scheme_cards_ui()
        self._refresh_pill_composer_ui()

    def _refresh_scheme_cards_ui(self):
        cur = str(self.cfg.get("input_scheme", "yinxing")).strip().lower()
        if cur not in ("quanpin", "shuangpin", "yinxing"):
            cur = "yinxing"
        if hasattr(self, "_scheme_cards"):
            for k, item in self._scheme_cards.items():
                card_f, t_lbl, sub_lbl, act_btn, acc_pair, tint_pair = item
                sel = (k == cur)
                card_f.configure(
                    fg_color=tint_pair if sel else C("surface"),
                    border_color=acc_pair if sel else C("border"),
                    border_width=2 if sel else 1,
                )
                t_lbl.configure(text_color=acc_pair if sel else C("text"))
                sub_lbl.configure(text_color=C("text") if sel else C("text_2"))
                if act_btn is not None:
                    act_btn.configure(
                        text="● 当前正在使用" if sel else "启用此方案",
                        fg_color=acc_pair if sel else C("surface_alt"),
                        hover_color=acc_pair if sel else C("nav_hover"),
                        text_color="#ffffff" if sel else C("text"),
                        border_width=0 if sel else 1,
                    )
        if hasattr(self, "page_size_seg"):
            ps = max(5, min(10, int(self.cfg.get("page_size", 10 if cur == "quanpin" else 5))))
            ps_lbl_map = {
                5: "5 个",
                6: "6 个",
                7: "7 个",
                8: "8 个",
                9: "9 个",
                10: "10 个 (1-9, 0 双手)",
            }
            want_ps_lbl = ps_lbl_map.get(ps, "5 个")
            if self.page_size_seg.get() != want_ps_lbl:
                self.page_size_seg.set(want_ps_lbl)

    def _save_pill_identity_from_ui(self, entry_widget=None):
        w = entry_widget or getattr(self, "gen_uname_entry", None)
        if w is None:
            return
        uname = w.get().strip()[:16]
        self._set_cfg("user_name", uname)
        if hasattr(self, "gen_uname_entry") and w is not self.gen_uname_entry:
            self.gen_uname_entry.delete(0, "end")
            if uname:
                self.gen_uname_entry.insert(0, uname)
        if hasattr(self, "sec_uname_entry") and w is not self.sec_uname_entry:
            self.sec_uname_entry.delete(0, "end")
            if uname:
                self.sec_uname_entry.insert(0, uname)
        disp = f"「{uname}」" if uname else "默认「鹤」字"
        messagebox.showinfo("胶囊用户名已更新", f"悬浮状态胶囊现已更新为显示 {disp}。", parent=self)

    def _upload_pill_avatar_dialog(self):
        from tkinter import filedialog
        path = filedialog.askopenfilename(
            parent=self,
            title="选择状态胶囊自定义头像图片",
            filetypes=[("图片文件", "*.png;*.jpg;*.jpeg;*.webp;*.bmp;*.ico"), ("所有文件", "*.*")],
        )
        if not path:
            return
        if _save_pill_avatar_files(path):
            self._set_cfg("avatar_enabled", True)
            for lbl_attr in ("gen_av_status_lbl", "sec_av_status_lbl", "wiz_av_status_lbl"):
                lbl = getattr(self, lbl_attr, None)
                if lbl is not None:
                    lbl.configure(text="✓ 已启用自定义头像", text_color=C("success"))
        else:
            messagebox.showwarning("头像上传失败", "无法解析所选图片文件，请选择有效的 PNG / JPG 图片。", parent=self)

    def _clear_pill_avatar(self):
        self._set_cfg("avatar_enabled", False)
        for lbl_attr in ("gen_av_status_lbl", "sec_av_status_lbl", "wiz_av_status_lbl"):
            lbl = getattr(self, lbl_attr, None)
            if lbl is not None:
                lbl.configure(text="当前显示文字徽章", text_color=C("muted"))

    def _on_avatar_size_slider(self, value):
        sz = int(round(float(value)))
        sz = max(16, min(36, sz))
        if hasattr(self, "lbl_gen_av_sz"):
            self.lbl_gen_av_sz.configure(text=f"{sz} px")
        if hasattr(self, "lbl_sec_av_sz"):
            self.lbl_sec_av_sz.configure(text=f"{sz} px")
        self._debounced("avatar_size", 180, lambda: self._set_cfg("avatar_size", sz))

    def _has_master_password(self) -> bool:
        return bool(str(self.cfg.get("master_pwd_verifier", "")).strip())

    def _has_history_pin(self) -> bool:
        return bool(self.cfg.get("history_pin_salt")) and bool(self.cfg.get("history_pin_verifier"))

    def _on_history_retention_change(self, mins):
        mins = int(mins)
        self._set_cfg("history_retention_mins", mins)
        if mins <= 0:
            for d in SYNC_DATA_DIRS:
                _vault_secure_wipe(os.path.join(d, "input_history.enc"))
        self._refresh_history_box()

    def _on_history_max_change(self, max_items):
        self._set_cfg("history_max_items", int(max_items))
        self._refresh_history_box()

    def _refresh_history_box(self):
        if not hasattr(self, "history_box"):
            return
        has_pin = self._has_history_pin()
        if has_pin:
            if self._pin_unlocked:
                self.pin_status_lbl.configure(text="🔓 PIN 已解锁", text_color=C("success"))
                if self.pin_unlock_btn.winfo_ismapped():
                    self.pin_unlock_btn.pack_forget()
            else:
                self.pin_status_lbl.configure(text="🔒 已受 PIN 码保护", text_color=C("accent"))
                if not self.pin_unlock_btn.winfo_ismapped():
                    self.pin_unlock_btn.pack(side="left", padx=(0, 8), before=self.pin_set_btn)
            self.pin_set_btn.configure(text="🔑 修改 / 关闭 PIN")
        else:
            self.pin_status_lbl.configure(text="本机密钥自动保护中", text_color=C("muted"))
            if self.pin_unlock_btn.winfo_ismapped():
                self.pin_unlock_btn.pack_forget()
            self.pin_set_btn.configure(text="🔑 设置 PIN 码")

        self.history_box.configure(state="normal")
        self.history_box.delete("1.0", "end")
        ret = int(self.cfg.get("history_retention_mins", 0))
        if ret <= 0:
            self.hist_count_lbl.configure(text="本地临时历史已关闭（默认用完即毁 · 零残留模式）")
            self.history_box.insert("end", "当前为默认「用完即毁」零残留模式：软件不记录任何打字历史，也不保留任何语音音频数据。")
            self.history_box.configure(state="disabled")
            return
        if has_pin and not self._pin_unlocked:
            self.hist_count_lbl.configure(text="🔒 历史记录已加密锁定")
            self.history_box.insert("end", "当前历史保险箱受个人 PIN 码保护。请点击上方「🔓 解锁查看」输入 PIN 码后查看或复制。")
            self.history_box.configure(state="disabled")
            return
        items = _vault_read_history(
            DATA_DIR,
            str(self.cfg.get("history_pin_salt", "")),
            str(self.cfg.get("history_wrapped_key", "")),
            ret,
            int(self.cfg.get("history_max_items", 200)),
        )
        self.hist_count_lbl.configure(text=f"最近临时记录（共 {len(items)} 条 · 满 {ret} 分钟自动永久销毁）")
        if not items:
            self.history_box.insert("end", "暂无临时输入历史记录。打字或语音上屏后会自动加密暂存于此，超时自动全零覆写销毁。")
        else:
            lines = []
            for ts, src, txt in items:
                tm_s = time.strftime("%H:%M:%S", time.localtime(ts))
                lines.append(f"[{tm_s} · {src}]  {txt}")
            self.history_box.insert("end", "\n".join(lines))
        self.history_box.configure(state="disabled")

    def _unlock_pin_dialog(self):
        dlg = ctk.CTkInputDialog(text="请输入个人 PIN 码以解锁查看本地加密历史：", title="解锁加密历史保险箱")
        pin = dlg.get_input()
        if pin is None:
            return
        if _vault_verify_pin(pin.strip(), str(self.cfg.get("history_pin_salt", "")), str(self.cfg.get("history_pin_verifier", ""))):
            self._pin_unlocked = True
            self._refresh_history_box()
        else:
            messagebox.showwarning("PIN 码错误", "输入的 PIN 码不正确，无法解密历史记录。", parent=self)

    def _open_pin_dialog(self):
        if self._has_history_pin() and not self._pin_unlocked:
            dlg0 = ctk.CTkInputDialog(text="请先输入当前 PIN 码进行验证：", title="验证当前 PIN 码")
            cur_pin = dlg0.get_input()
            if cur_pin is None:
                return
            if not _vault_verify_pin(cur_pin.strip(), str(self.cfg.get("history_pin_salt", "")), str(self.cfg.get("history_pin_verifier", ""))):
                messagebox.showwarning("PIN 码错误", "当前 PIN 码验证失败。", parent=self)
                return
            self._pin_unlocked = True

        dlg = ctk.CTkInputDialog(text="输入新 PIN 码（4~16 位数字或字母；留空并确定可关闭 PIN 码锁）：", title="设置加密历史 PIN 码")
        new_pin = dlg.get_input()
        if new_pin is None:
            return
        new_pin = new_pin.strip()
        old_items = _vault_read_history(
            DATA_DIR,
            str(self.cfg.get("history_pin_salt", "")),
            str(self.cfg.get("history_wrapped_key", "")),
            int(self.cfg.get("history_retention_mins", 0)),
            int(self.cfg.get("history_max_items", 200)),
        )
        if not new_pin:
            self.cfg["history_pin_salt"] = ""
            self.cfg["history_pin_verifier"] = ""
            self.cfg["history_wrapped_key"] = ""
            self._cfg_touched.update(("history_pin_salt", "history_pin_verifier", "history_wrapped_key"))
            self._save_config()
            _vault_write_history(SYNC_DATA_DIRS, "", "", old_items)
            self._pin_unlocked = False
            self._refresh_history_box()
            messagebox.showinfo("PIN 码已关闭", "已关闭独立 PIN 码锁，恢复为本机专属密钥自动加密保护。", parent=self)
            return
        salt_h, ver_h, wrap_h = _vault_create_pin_params(DATA_DIR, new_pin)
        self.cfg["history_pin_salt"] = salt_h
        self.cfg["history_pin_verifier"] = ver_h
        self.cfg["history_wrapped_key"] = wrap_h
        self._cfg_touched.update(("history_pin_salt", "history_pin_verifier", "history_wrapped_key"))
        self._save_config()
        _vault_write_history(SYNC_DATA_DIRS, salt_h, wrap_h, old_items)
        self._pin_unlocked = True
        self._refresh_history_box()
        messagebox.showinfo("PIN 码已生效", "已使用 PBKDF2-HMAC-SHA256 (10,000 轮) 更新加密密钥并重新加密历史保险箱。", parent=self)

    def _copy_all_history(self):
        if self._has_history_pin() and not self._pin_unlocked:
            self._unlock_pin_dialog()
            if not self._pin_unlocked:
                return
        items = _vault_read_history(
            DATA_DIR,
            str(self.cfg.get("history_pin_salt", "")),
            str(self.cfg.get("history_wrapped_key", "")),
            int(self.cfg.get("history_retention_mins", 0)),
            int(self.cfg.get("history_max_items", 200)),
        )
        if not items:
            messagebox.showinfo("复制历史", "当前没有可复制的临时历史记录。", parent=self)
            return
        merged = "".join(txt for _, _, txt in reversed(items))
        self.clipboard_clear()
        self.clipboard_append(merged)
        self.hist_count_lbl.configure(text=f"✓ 已合并复制 {len(items)} 条（共 {len(merged)} 字）到剪贴板")

    def _wipe_history_now(self):
        for d in SYNC_DATA_DIRS:
            _vault_secure_wipe(os.path.join(d, "input_history.enc"))
        self._refresh_history_box()
        if hasattr(self, "hist_count_lbl"):
            self.hist_count_lbl.configure(text="🔥 已全零覆写并永久销毁全部本地输入历史（零残留）")

    def _cfg_switch(self, parent, key):
        default_val = key != "hide_candidate_hud"
        holder = {}

        def _cmd():
            self._set_cfg(key, bool(holder["sw"].get()))

        sw = self._switch(parent, bool(self.cfg.get(key, default_val)), _cmd)
        holder["sw"] = sw
        self.__dict__[f"_sw_{key}"] = sw
        return sw

    def _on_font_slider(self, value):
        self._apply_unified_font_scale(float(value), source="general")

    def _apply_unified_font_scale(self, raw_val: float, source: str = ""):
        v = round(max(0.75, min(2.20, float(raw_val))) * 20) / 20
        pct = f"{int(round(v * 100))}%"
        if hasattr(self, "lbl_font_scale"):
            self.lbl_font_scale.configure(text=pct)
        if hasattr(self, "font_slider") and source != "general":
            self.font_slider.set(v)
        if hasattr(self, "lbl_app_font_scale"):
            self.lbl_app_font_scale.configure(text=pct)
        if hasattr(self, "app_font_slider") and source != "appearance":
            self.app_font_slider.set(v)
        if hasattr(self, "_update_live_hud_preview"):
            self._update_live_hud_preview(v)
        self._debounced("font_scale", 200, lambda: self._set_cfg("font_scale", round(v, 2)))

    def _refresh_telemetry(self):
        if not hasattr(self, "_tm"):
            return
        candidates = [os.path.join(d, "telemetry.json") for d in SYNC_DATA_DIRS if os.path.exists(os.path.join(d, "telemetry.json"))]
        if not candidates:
            return
        newest = max(candidates, key=lambda p: os.path.getmtime(p))
        tel = _read_json(newest, None)
        if not isinstance(tel, dict):
            return
        try:
            self._tm["cur"].configure(text=f"{tel.get('current_cpm', 0)} 字/分")
            self._tm["peak"].configure(text=f"{tel.get('peak_cpm', 0)} 字/分")
            self._tm["avg"].configure(text=f"{float(tel.get('avg_code_length', 0.0)):.2f} 键/字")
            self._tm["saved"].configure(text=f"{int(tel.get('saved_keystrokes', 0)):,} 次")
        except Exception:
            pass

    def _is_autostart_enabled(self) -> bool:
        try:
            import winreg
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Run", 0, winreg.KEY_READ) as k:
                val, _ = winreg.QueryValueEx(k, "HeyuIME")
                return bool(val)
        except Exception:
            return False

    def _toggle_autostart(self):
        try:
            import winreg
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Run", 0, winreg.KEY_SET_VALUE) as k:
                if self.sw_autostart.get():
                    winreg.SetValueEx(k, "HeyuIME", 0, winreg.REG_SZ, f'"{EXE_PATH}"')
                    if os.path.exists(VOICE_EXE_PATH):
                        winreg.SetValueEx(k, "HeyuVoice", 0, winreg.REG_SZ, f'"{VOICE_EXE_PATH}"')
                else:
                    for vname in ("HeyuIME", "HeyuVoice"):
                        try:
                            winreg.DeleteValue(k, vname)
                        except FileNotFoundError:
                            pass
        except Exception as e:
            messagebox.showwarning("开机自启", f"设置开机自启时出错: {e}")

    # =========================================================================
    # PAGE ② 词库 DICTIONARY (免密新增词条 + 主密码验证解锁查看全部词条 + HEYUENC1 密文落盘)
    # =========================================================================
    PARAM_SPECS = [
        ("{date}", "标准日期", "2026-10-19"),
        ("{date_zh}", "中文日期", "2026年10月19日"),
        ("{date_slash}", "斜杠日期", "2026/10/19"),
        ("{time}", "当前时间", "14:50"),
        ("{datetime}", "日期 + 时间", "2026-10-19 14:50"),
        ("{weekday}", "星期", "星期一"),
        ("{year}年{month}月", "年 / 月 / 日", "{year} {month} {day}"),
        ("\\n", "换行", "多行文本"),
    ]

    def _build_page_dict(self):
        page, col = self._page(
            "词库",
            "新增词条无需输入密码 · 查看全部词条受主密码保护 · 全盘 HEYUENC1 顶级密文加密存储",
            scroll=False,
            max_w=920,
        )

        # ---- Top editor card (Always open without password: 创建新词条不需要输入密码)
        ed = self._card(col)
        ed.pack(fill="x", pady=(10, 0))
        top_hdr = ctk.CTkFrame(ed, fg_color="transparent")
        top_hdr.pack(fill="x", padx=ROW_PX, pady=(12, 0))
        self._label(top_hdr, "➕ 免密快速添加新词条", 14, "bold", color="accent").pack(side="left")
        self._chip(top_hdr, "免密直添 · 自动加密写入", color="success", size=11, mono=False).pack(side="left", padx=(10, 0))
        self._btn(top_hdr, "💾 导出加密词库备份", self._export_encrypted_lexicon_backup, height=28).pack(side="right")

        form = ctk.CTkFrame(ed, fg_color="transparent")
        form.pack(fill="x", padx=ROW_PX, pady=(10, 0))
        form.grid_columnconfigure(1, weight=1)
        self._label(form, "编码", 12, "bold", color="muted").grid(row=0, column=0, sticky="w", pady=(0, 6))
        self._label(form, "词句（可包含动态参数或重要个人内容，落盘即自动密文加密）", 12, "bold", color="muted").grid(
            row=0, column=1, sticky="w", padx=(12, 0), pady=(0, 6)
        )
        self._code_manually_edited = False
        self.new_code_entry = self._entry(form, 150, "自动生成或手动填写", mono=True)
        self.new_code_entry.grid(row=1, column=0, sticky="w")
        self.new_word_entry = self._entry(form, 300, "例如 达到 / 帮我 / 片语工作室")
        self.new_word_entry.grid(row=1, column=1, sticky="ew", padx=(12, 0))
        self.new_code_entry.bind("<KeyRelease>", lambda e: setattr(self, "_code_manually_edited", bool(self.new_code_entry.get().strip())))
        self.new_word_entry.bind("<KeyRelease>", lambda e: self._dict_update_preview())
        self.new_code_entry.bind("<Return>", lambda e: self.new_word_entry.focus_set())
        self.new_word_entry.bind("<Return>", lambda e: self._dict_commit())

        act = ctk.CTkFrame(ed, fg_color="transparent")
        act.pack(fill="x", padx=ROW_PX, pady=(10, 0))
        self.new_mode_var = ctk.StringVar(value="直出")
        self.new_mode_seg = self._segmented(act, ["直出", "候选首选"], lambda v: self.new_mode_var.set(v))
        self.new_mode_seg.set("直出")
        self.new_mode_seg.pack(side="left")
        self.dict_preview = self._label(act, "", 12, color="muted")
        self.dict_preview.pack(side="left", padx=(16, 8), fill="x", expand=True)
        self.dict_commit_btn = self._btn(act, "添加词条", self._dict_commit, kind="primary", width=104)
        self.dict_commit_btn.pack(side="right")
        self.dict_delete_btn = self._btn(act, "删除", self._dict_delete, kind="danger")
        self.dict_clear_btn = self._btn(act, "清空", self._dict_clear_editor)
        self.dict_clear_btn.pack(side="right", padx=(0, 8))

        # collapsible parameter guide
        tog = ctk.CTkFrame(ed, fg_color="transparent")
        tog.pack(fill="x", padx=ROW_PX - 8, pady=(8, 8))
        self._param_open = False
        self.param_toggle = ctk.CTkButton(
            tog, text="▸  动态参数写法", command=self._toggle_params, height=26, width=10,
            fg_color="transparent", hover_color=C("surface_alt"), text_color=C("accent"), font=self.F(13), anchor="w",
        )
        self.param_toggle.pack(side="left")
        self._label(tog, "在词句中插入日期、时间等，上屏瞬间自动替换", 12, color="muted").pack(side="left", padx=(4, 0))

        self.param_panel = ctk.CTkFrame(ed, fg_color="transparent")
        grid = ctk.CTkFrame(self.param_panel, fg_color="transparent")
        grid.pack(fill="x", padx=ROW_PX, pady=(0, 4))
        for c in range(4):
            grid.grid_columnconfigure(c, weight=1, uniform="pg")
        for i, (token, name, sample) in enumerate(self.PARAM_SPECS):
            cell = ctk.CTkFrame(grid, fg_color="transparent")
            cell.grid(row=i // 4, column=i % 4, sticky="w", pady=6)
            ctk.CTkButton(cell, text=token, command=lambda t=token: self._insert_param_to_word_entry(t),
                          height=26, width=self._text_w(token, self.F(12, mono=True)) + 20, corner_radius=6,
                          font=self.F(12, mono=True), fg_color=C("accent_soft"), hover_color=C("nav_hover"),
                          text_color=C("accent")).pack(anchor="w")
            self._label(cell, f"{name} · {sample}", 12, color="muted").pack(anchor="w", pady=(4, 0))
        self._label(self.param_panel, "打字时也可直接输入 orq（日期）· ouj（时间）· oxq（星期）呼出时间宏。点击参数即可插入到词句光标处。",
                    12, color="muted", wraplength=820).pack(anchor="w", padx=ROW_PX, pady=(4, 14))

        # ---- Bottom card: Password Gate OR Unlocked Table (查看词库所有词条必须输入密码)
        self.dict_vault_host = ctk.CTkFrame(col, fg_color="transparent")
        self.dict_vault_host.pack(fill="both", expand=True, pady=(14, 0))

        # 1) Locked Gate Frame
        self.dict_lock_card = self._card(self.dict_vault_host)
        lock_inner = ctk.CTkFrame(self.dict_lock_card, fg_color="transparent")
        lock_inner.pack(fill="both", expand=True, padx=32, pady=28)
        self._label(lock_inner, "🔒 词库词条受顶级密文加密保护", 17, "bold", color="accent").pack(anchor="w")
        self._label(
            lock_inner,
            "您可能会在词库中保存重要信息或私密快捷长句。为确保绝对安全，所有词库文件在硬盘上均以 HEYUENC1 (256-bit) 认证密文存储。\n"
            "• 在上方创建添加新词条无需输入密码，随时免密直添；\n"
            "• 查看、搜索或编辑词库中的所有词条，必须先验证您的安全主密码。",
            13,
            color="text_2",
            wraplength=780,
        ).pack(anchor="w", pady=(8, 16))
        pw_row = ctk.CTkFrame(lock_inner, fg_color="transparent")
        pw_row.pack(anchor="w")
        self.dict_pwd_entry = self._entry(pw_row, 260, "输入安全主密码以解锁查看全部词条…")
        self.dict_pwd_entry.configure(show="•")
        self.dict_pwd_entry.pack(side="left", padx=(0, 10))
        self.dict_pwd_entry.bind("<Return>", lambda e: self._unlock_dict_with_password())
        self._btn(pw_row, "🔓 验证主密码并查看词库", self._unlock_dict_with_password, kind="primary", height=36).pack(side="left")
        self.dict_lock_msg = self._label(lock_inner, "", 12, "bold", color="danger")
        self.dict_lock_msg.pack(anchor="w", pady=(10, 0))

        # 2) Unlocked Table Card
        tc = self._card(self.dict_vault_host)
        self.dict_table_card = tc
        bar = ctk.CTkFrame(tc, fg_color="transparent")
        bar.pack(fill="x", padx=ROW_PX, pady=(14, 10))
        self._label(bar, "我的加密词库全部词条", 15, "bold").pack(side="left")
        self.dict_count = self._chip(bar, "0", mono=True)
        self.dict_count.pack(side="left", padx=(10, 0))
        self._btn(bar, "🔒 立即锁定", self._lock_dict_view, height=30).pack(side="right", padx=(8, 0))
        self.dict_search = self._entry(bar, 220, "搜索编码或词句")
        self.dict_search.pack(side="right")
        self.dict_search.bind("<KeyRelease>", lambda e: self._dict_refresh_tree())

        holder = ctk.CTkFrame(tc, fg_color="transparent")
        holder.pack(fill="both", expand=True, padx=(ROW_PX - 8, 8), pady=(0, 4))
        self.dict_tree = self._make_tree(holder, [("code", "编码", 130, False), ("word", "词句", 320, True),
                                                  ("mode", "上屏方式", 100, False), ("preview", "上屏预览", 240, True)])
        self.dict_tree.bind("<<TreeviewSelect>>", lambda e: self._dict_on_select())
        self.dict_tree.bind("<Delete>", lambda e: self._dict_delete())

        foot = ctk.CTkFrame(tc, fg_color="transparent")
        foot.pack(fill="x", padx=ROW_PX, pady=(4, 12))
        self._label(foot, "选中条目即可在上方编辑 · Delete 键删除 · 所有词条均以 HEYUENC1 密文同步存储", 12, color="muted").pack(side="left")
        self.table_status_lbl = self._label(foot, "", 12, "bold", color="success")
        self.table_status_lbl.pack(side="right")

        self._dict_edit_idx = None
        self._dict_headers, self._dict_entries = self._read_user_dict_entries()
        self._dict_refresh_tree()
        self._dict_update_preview()
        self._refresh_dict_lock_view()
        return page

    def _on_show_dict(self):
        self._refresh_dict_lock_view()
        self.after(50, lambda: self.new_code_entry.focus_set())

    def _refresh_dict_lock_view(self):
        if not hasattr(self, "dict_lock_card") or not hasattr(self, "dict_table_card"):
            return
        self._dict_headers, self._dict_entries = self._read_user_dict_entries()
        if self._has_master_password() and not self._dict_unlocked:
            self.dict_table_card.pack_forget()
            self.dict_lock_card.pack(fill="both", expand=True)
            if hasattr(self, "dict_count"):
                self.dict_count.configure(text=f"🔒 已加密 ({len(self._dict_entries)} 条)")
        else:
            self.dict_lock_card.pack_forget()
            self.dict_table_card.pack(fill="both", expand=True)
            self._dict_refresh_tree()

    def _unlock_dict_with_password(self):
        pwd = self.dict_pwd_entry.get() if hasattr(self, "dict_pwd_entry") else ""
        if not pwd:
            self.dict_lock_msg.configure(text="⚠ 请输入安全主密码。")
            return
        ok, vkey = _master_verify_password(DATA_DIR, pwd, self.cfg)
        if not ok or not vkey:
            self.dict_lock_msg.configure(text="❌ 主密码不正确，请重试。")
            return
        self._unlocked_vault_key = vkey
        self._dict_unlocked = True
        self.dict_pwd_entry.delete(0, "end")
        self.dict_lock_msg.configure(text="")
        # Authorize this device if needed & update last verified timestamp
        cur_dev = _compute_device_id(DATA_DIR)
        k_dev = _compute_device_wrap_key(DATA_DIR)
        self.cfg["authorized_device_id"] = cur_dev
        self.cfg["device_wrapped_vault_key"] = _vault_encrypt(k_dev, vkey.hex())
        self.cfg["last_password_verified_ts"] = int(time.time())
        self._cfg_touched.update(("authorized_device_id", "device_wrapped_vault_key", "last_password_verified_ts"))
        self._save_config()
        self._refresh_dict_lock_view()

    def _lock_dict_view(self):
        self._dict_unlocked = False
        self._dict_clear_editor()
        self._refresh_dict_lock_view()

    def _toggle_params(self):
        self._param_open = not self._param_open
        if self._param_open:
            self.param_panel.pack(fill="x", after=self.param_toggle.master)
            self.param_toggle.configure(text="▾  动态参数写法")
        else:
            self.param_panel.pack_forget()
            self.param_toggle.configure(text="▸  动态参数写法")

    def _expand_preview(self, text: str) -> str:
        now = time.localtime()
        wd = ["星期一", "星期二", "星期三", "星期四", "星期五", "星期六", "星期日"][now.tm_wday]
        y, m, d, hh, mm, ss = now.tm_year, now.tm_mon, now.tm_mday, now.tm_hour, now.tm_min, now.tm_sec
        return (
            text.replace("{date}", f"{y:04d}-{m:02d}-{d:02d}")
            .replace("{date_zh}", f"{y}年{m}月{d}日")
            .replace("{date_slash}", f"{y:04d}/{m:02d}/{d:02d}")
            .replace("{date_compact}", f"{y:04d}{m:02d}{d:02d}")
            .replace("{time}", f"{hh:02d}:{mm:02d}")
            .replace("{time_sec}", f"{hh:02d}:{mm:02d}:{ss:02d}")
            .replace("{datetime}", f"{y:04d}-{m:02d}-{d:02d} {hh:02d}:{mm:02d}")
            .replace("{weekday}", wd)
            .replace("{year}", f"{y:04d}")
            .replace("{month}", f"{m:02d}")
            .replace("{day}", f"{d:02d}")
            .replace("\\n", " ↵ ")
        )

    def _insert_param_to_word_entry(self, token: str):
        try:
            self.new_word_entry.insert("insert", token)
            self.new_word_entry.focus_set()
            self._dict_update_preview()
        except Exception:
            pass

    def _suggest_phrase_code_py(self, text: str) -> str:
        rev = getattr(self, "_quick_reverse_map", None)
        if rev is None:
            rev = {}
            for name in ("heyu_reverse.tsv", "flypy_reverse.tsv", "heyu_open_reverse.tsv"):
                p = os.path.join(DATA_DIR, name)
                if os.path.exists(p):
                    try:
                        with open(p, "r", encoding="utf-8") as f:
                            for ln in f:
                                if ln.startswith("#") or "\t" not in ln:
                                    continue
                                ch, codes = ln.strip().split("\t", 1)
                                for c in codes.split(","):
                                    c = c.strip().lower()
                                    if len(c) >= 2 and c.isalpha():
                                        rev[ch] = c
                                        break
                    except Exception:
                        pass
                    if rev:
                        break
            self._quick_reverse_map = rev
        char_codes = [rev[ch] for ch in text if not ch.isascii() and ch in rev]
        n = len(char_codes)
        if n == 0:
            return ""
        if n == 1:
            return char_codes[0]
        if n == 2:
            return char_codes[0][:2] + char_codes[1][:2]
        if n == 3:
            return char_codes[0][:1] + char_codes[1][:1] + char_codes[2][:2]
        return char_codes[0][:1] + char_codes[1][:1] + char_codes[2][:1] + char_codes[-1][:1]

    def _dict_update_preview(self):
        w = self.new_word_entry.get().strip()
        if not getattr(self, "_code_manually_edited", False) and self._dict_edit_idx is None:
            sug = self._suggest_phrase_code_py(w) if w else ""
            self.new_code_entry.delete(0, "end")
            if sug:
                self.new_code_entry.insert(0, sug)
        if not w:
            self.dict_preview.configure(text="上屏预览会显示在这里", text_color=C("muted"))
            return
        p = self._expand_preview(w)
        if len(p) > 40:
            p = p[:39] + "…"
        self.dict_preview.configure(text=f"预览  {p}", text_color=C("success"))

    def _resolve_active_lexicon_key(self):
        if getattr(self, "_unlocked_vault_key", None):
            return self._unlocked_vault_key
        st, k = _resolve_authorized_lexicon_key_py(DATA_DIR, self.cfg)
        if st == "ok" and k:
            return k
        if st == "unconfigured":
            secret = _vault_load_secret(DATA_DIR)
            return hmac.new(secret, b"heyu-default-lexicon-key-v1", hashlib.sha256).digest()
        return None

    DEFAULT_STARTER_USER_ENTRIES = [
        ("pygu", "片语工作室", "直出"),
        ("hy", "鹤羽", "个人专属纯净输入法"),
        ("hyyx", "鹤羽音形", "净室离线极速引擎"),
        ("cyyf", "您好，我已经收到您的消息，稍后给您详细回复。", "直出"),
        ("wddz", "Via Roma 1, 00100 Roma RM, Italia", "直出"),
        ("qmri", "—— 签署于 {date_zh} ({weekday}) {time}", "直出"),
    ]

    def _read_user_dict_entries(self):
        headers = []
        entries = []
        seen_pairs = set()
        needs_reencrypt = False
        saw_encrypted_block = False
        active_k = self._resolve_active_lexicon_key()
        secret = _vault_load_secret(DATA_DIR)
        k_def = hmac.new(secret, b"heyu-default-lexicon-key-v1", hashlib.sha256).digest()

        cand_paths = []
        for p in (
            USER_DICT_PATH,
            os.path.join(LOCAL_DATA_DIR, "user_custom.tsv"),
            os.path.join(APPDATA_DATA_DIR, "user_custom.tsv") if APPDATA_DATA_DIR else "",
            os.path.join(INSTALL_DIR, "data", "user_custom.tsv"),
        ):
            if p and p not in cand_paths and os.path.exists(p):
                cand_paths.append(p)
        # Always read the most recently modified user_custom.tsv first (e.g. written by Rust Quick-Add or Engine)
        cand_paths.sort(key=lambda x: os.path.getmtime(x) if os.path.exists(x) else 0.0, reverse=True)

        for idx_p, path in enumerate(cand_paths):
            # Only merge secondary fallback files if the newest file had 0 entries
            if idx_p > 0 and entries:
                break
            expanded_lines = []
            try:
                with open(path, "r", encoding="utf-8") as f:
                    content = f.read()
                for raw_ln in content.splitlines():
                    ln = raw_ln.strip()
                    if not ln:
                        continue
                    if ln.startswith("#"):
                        expanded_lines.append(ln)
                        continue
                    if ln.startswith("HEYUENC1:"):
                        saw_encrypted_block = True
                        dec = _vault_decrypt(active_k, ln) if active_k is not None else None
                        if dec is None:
                            dec2 = _vault_decrypt(k_def, ln)
                            if dec2 is not None:
                                dec = dec2
                                needs_reencrypt = True
                        if dec is not None:
                            expanded_lines.extend(dec.splitlines())
                    else:
                        expanded_lines.append(raw_ln)
                        needs_reencrypt = True
            except Exception:
                pass

            for line in expanded_lines:
                s = line.rstrip("\r\n")
                if s.startswith("#"):
                    if s not in headers and len(headers) < 4:
                        headers.append(s)
                    continue
                if not s.strip():
                    continue
                parts = s.split("\t")
                if len(parts) >= 2:
                    code = parts[0].strip()
                    word = parts[1].strip()
                    comment = parts[2].strip() if len(parts) >= 3 else "直出"
                    if code and word and (code, word) not in seen_pairs:
                        seen_pairs.add((code, word))
                        entries.append((code, word, comment))
                        if idx_p > 0:
                            needs_reencrypt = True

        # Only populate starter entries when no encrypted block exists on disk and dictionary wasn't intentionally emptied
        if not entries and not saw_encrypted_block and not bool(self.cfg.get("user_dict_intentionally_emptied", False)):
            entries = list(self.DEFAULT_STARTER_USER_ENTRIES)
            needs_reencrypt = True

        if not headers:
            headers = [
                "# HEYU Personal Custom Lexicon (个人专属置顶词库 · 顶级密文加密保护)",
                "# Encrypted with 256-bit CTR-SHA256 + HMAC-SHA256 (PBKDF2 120,000 iterations)",
            ]
        if needs_reencrypt and active_k is not None and (entries or not saw_encrypted_block):
            self._write_user_dict_entries(headers, entries)
        return headers, entries

    def _write_user_dict_entries(self, headers, entries):
        plain_lines = []
        for code, word, comment in entries:
            c = code.strip().lower()
            w = word.strip()
            m = comment.strip() or "直出"
            if c and w:
                plain_lines.append(f"{c}\t{w}\t{m}")
        plain_body = ("\n".join(plain_lines) + "\n") if plain_lines else ""
        k = self._resolve_active_lexicon_key()
        hdr_block = (
            "# HEYU Personal Custom Lexicon (个人专属置顶词库 · 顶级密文加密保护)\n"
            "# Encrypted with 256-bit CTR-SHA256 + HMAC-SHA256 (PBKDF2 120,000 iterations)\n"
        )
        if k is not None:
            payload = hdr_block + ((_vault_encrypt(k, plain_body) + "\n") if plain_body else "")
        else:
            payload = hdr_block + plain_body
        for d in SYNC_DATA_DIRS:
            try:
                os.makedirs(d, exist_ok=True)
                with open(os.path.join(d, "user_custom.tsv"), "w", encoding="utf-8", newline="\n") as f:
                    f.write(payload)
            except Exception:
                pass
        try:
            hwnd = ctypes.windll.user32.FindWindowW("HeyuLayeredWindowClass", "HeyuStatusPillWindow")
            if hwnd:
                ctypes.windll.user32.PostMessageW(hwnd, 0x0400 + 204, 0, 0)
        except Exception:
            pass

    @staticmethod
    def _is_auto(comment):
        return ("直出" in comment) or ("独占" in comment) or (comment.lower() == "auto")

    def _dict_refresh_tree(self, select_idx=None):
        if not hasattr(self, "dict_tree"):
            return
        tree = self.dict_tree
        tree.delete(*tree.get_children())
        if self._has_master_password() and not self._dict_unlocked:
            self.dict_count.configure(text="🔒 已加密")
            return
        q = self.dict_search.get().strip().lower() if hasattr(self, "dict_search") else ""
        shown = 0
        for idx, (code, word, comment) in enumerate(self._dict_entries):
            if q and q not in code.lower() and q not in word.lower():
                continue
            mode = "直出" if self._is_auto(comment) else "候选首选"
            prev = self._expand_preview(word)
            tree.insert("", "end", iid=str(idx), values=(f" {code}", f" {word}", f" {mode}", f" {prev}"),
                        tags=("odd",) if shown % 2 else ())
            shown += 1
        self.dict_count.configure(text=str(len(self._dict_entries)))
        if select_idx is not None and tree.exists(str(select_idx)):
            tree.selection_set(str(select_idx))
            tree.see(str(select_idx))

    def _dict_on_select(self):
        if self._has_master_password() and not self._dict_unlocked:
            return
        sel = self.dict_tree.selection()
        if not sel:
            return
        idx = int(sel[0])
        if idx >= len(self._dict_entries):
            return
        code, word, comment = self._dict_entries[idx]
        self._dict_edit_idx = idx
        self.new_code_entry.delete(0, "end")
        self.new_code_entry.insert(0, code)
        self.new_word_entry.delete(0, "end")
        self.new_word_entry.insert(0, word)
        mode = "直出" if self._is_auto(comment) else "候选首选"
        self.new_mode_seg.set(mode)
        self.new_mode_var.set(mode)
        self.dict_commit_btn.configure(text="保存修改")
        if not self.dict_delete_btn.winfo_ismapped():
            self.dict_delete_btn.pack(side="right", padx=(0, 8), before=self.dict_clear_btn)
        self._dict_update_preview()

    def _dict_clear_editor(self):
        self._dict_edit_idx = None
        self._code_manually_edited = False
        self.new_code_entry.delete(0, "end")
        self.new_word_entry.delete(0, "end")
        self.new_mode_seg.set("直出")
        self.new_mode_var.set("直出")
        self.dict_commit_btn.configure(text="添加词条")
        self.dict_delete_btn.pack_forget()
        if hasattr(self, "dict_tree"):
            sel = self.dict_tree.selection()
            if sel:
                self.dict_tree.selection_remove(*sel)
        self._dict_update_preview()
        self.new_code_entry.focus_set()

    def _dict_commit(self):
        code = self.new_code_entry.get().strip().lower()
        word = self.new_word_entry.get().strip()
        if not code and word:
            code = self._suggest_phrase_code_py(word)
        mode = self.new_mode_var.get()
        if not code or not word:
            messagebox.showwarning("提示", "请填写编码（如 pygu）和词句（如 片语工作室）。", parent=self)
            return
        if self._resolve_active_lexicon_key() is None:
            messagebox.showwarning("设备未授权", "检测到已更换电脑设备，请先验证主密码以授权本设备后再写入词库。", parent=self)
            return
        headers, entries = self._read_user_dict_entries()
        if self._dict_edit_idx is not None and self._dict_edit_idx < len(self._dict_entries):
            old = self._dict_entries[self._dict_edit_idx]
            pos = next((i for i, e in enumerate(entries) if e == old), self._dict_edit_idx)
            if pos < len(entries):
                entries[pos] = (code, word, mode)
            else:
                entries.insert(0, (code, word, mode))
            msg = f"✓ 已加密保存：{code} → {word}"
        else:
            entries = [(c, w, m) for (c, w, m) in entries if not (c == code and w == word)]
            entries.insert(0, (code, word, mode))
            msg = f"✓ 已免密添加并加密：{code} → {word}"
        if self.cfg.get("user_dict_intentionally_emptied"):
            self._set_cfg("user_dict_intentionally_emptied", False)
        self._write_user_dict_entries(headers, entries)
        self._dict_headers, self._dict_entries = self._read_user_dict_entries()
        self._dict_clear_editor()
        self._dict_refresh_tree(select_idx=None)
        if hasattr(self, "table_status_lbl"):
            self.table_status_lbl.configure(text=msg)
        if self._has_master_password() and not self._dict_unlocked:
            self.dict_preview.configure(text=msg, text_color=C("success"))

    def _dict_delete(self):
        if self._has_master_password() and not self._dict_unlocked:
            return
        sel = self.dict_tree.selection()
        idx = int(sel[0]) if sel else self._dict_edit_idx
        if idx is None or idx >= len(self._dict_entries):
            return
        code, word, _ = self._dict_entries[idx]
        if not messagebox.askyesno("删除词条", f"确定删除「{code} → {word}」？", parent=self):
            return
        headers, entries = self._read_user_dict_entries()
        target = self._dict_entries[idx]
        entries = [e for e in entries if e != target] if target in entries else entries
        if not entries:
            self._set_cfg("user_dict_intentionally_emptied", True)
        self._write_user_dict_entries(headers, entries)
        self._dict_headers, self._dict_entries = self._read_user_dict_entries()
        self._dict_clear_editor()
        self._dict_refresh_tree()
        self.table_status_lbl.configure(text=f"已删除：{code} → {word}")

    # ---- ttk.Treeview (fast tables) ------------------------------------------
    def _make_tree(self, parent, columns, height=10):
        self._ensure_tree_style()
        frame = ctk.CTkFrame(parent, fg_color="transparent")
        frame.pack(fill="both", expand=True)
        tree = ttk.Treeview(frame, columns=[c[0] for c in columns], show="headings", style="Heyu.Treeview",
                            selectmode="browse", height=height)
        for cid, title, width, stretch in columns:
            tree.heading(cid, text=f" {title}", anchor="w")
            tree.column(cid, width=int(width * self._ws()), minwidth=40, stretch=stretch, anchor="w")
        sb = ctk.CTkScrollbar(frame, command=tree.yview, button_color=C("border_strong"),
                              button_hover_color=C("muted"), fg_color="transparent", width=12)
        tree.configure(yscrollcommand=sb.set)
        sb.pack(side="right", fill="y", padx=(4, 0))
        tree.pack(side="left", fill="both", expand=True)
        self._trees.append(tree)
        self._apply_tree_style()
        return tree

    def _ensure_tree_style(self):
        if self._tree_style is not None:
            return
        s = ttk.Style(self)
        try:
            s.theme_use("clam")
        except Exception:
            pass
        s.layout("Heyu.Treeview", [("Heyu.Treeview.treearea", {"sticky": "nswe"})])
        self._tree_style = s
        self._tv_font = tkfont.Font(self, family=self.ui_family, size=-13)
        self._tv_head = tkfont.Font(self, family=self.ui_family, size=-12, weight="bold")

    def _apply_tree_style(self):
        if self._tree_style is None:
            return
        P = PALETTES["dark" if ctk.get_appearance_mode() == "Dark" else "light"]
        sc = self._ws()
        self._tv_font.configure(size=-round(13 * sc))
        self._tv_head.configure(size=-round(12 * sc))
        s = self._tree_style
        s.configure("Heyu.Treeview", background=P["surface"], fieldbackground=P["surface"], foreground=P["text"],
                    rowheight=round(34 * sc), font=self._tv_font, borderwidth=0, relief="flat")
        s.configure("Heyu.Treeview.Heading", background=P["surface"], foreground=P["muted"], font=self._tv_head,
                    relief="flat", borderwidth=1, bordercolor=P["border"], lightcolor=P["surface"],
                    darkcolor=P["border"], padding=(2, round(6 * sc)))
        s.map("Heyu.Treeview.Heading", background=[("active", P["surface_alt"])])
        s.map("Heyu.Treeview", background=[("selected", P["accent_soft"])], foreground=[("selected", P["text"])])
        for tree in self._trees:
            try:
                tree.tag_configure("odd", background=P["surface_alt"])
            except Exception:
                pass

    def _on_mode_changed(self, *_):
        self._apply_tree_style()
        self._apply_dual_text_theme()
        if hasattr(self, "sb_theme_btn"):
            is_dark = ctk.get_appearance_mode() == "Dark"
            if getattr(self, "mini_mode", None):
                self.sb_theme_btn.configure(text="", image=self._ui_icon("moon" if is_dark else "sun", (15, 15)))
            else:
                self.sb_theme_btn.configure(
                    text="🌙 深色模式 · 点击切换浅色" if is_dark else "☀ 浅色模式 · 点击切换深色"
                )
        col = self._mode_color("border")
        for h in self._hairlines:
            try:
                h.configure(bg=col)
            except Exception:
                pass

    def _toggle_sidebar_appearance(self):
        new_key = "light" if ctk.get_appearance_mode() == "Dark" else "dark"
        self._on_appearance_change(new_key)
        if hasattr(self, "appearance_seg"):
            lbl = next((l for l, k in _APPEARANCE_LABELS if k == new_key), None)
            if lbl:
                self.appearance_seg.set(lbl)

    def _mode_color(self, name):
        return PALETTES["dark" if ctk.get_appearance_mode() == "Dark" else "light"][name]

    def _hairline(self, parent):
        """1 physical-pixel divider (tk.Frame; recoloured on light/dark switch)."""
        h = tk.Frame(parent, height=1, bg=self._mode_color("border"), bd=0, highlightthickness=0)
        self._hairlines.append(h)
        return h

    # =========================================================================
    # PAGE ③ 语音输入 VOICE (Decluttered Settings + Standalone Mini-Window Launchers)
    # =========================================================================
    def _launch_mini_window(self, flag: str):
        try:
            subprocess.Popen(
                [sys.executable, os.path.abspath(__file__), flag],
                cwd=BASE_DIR,
                creationflags=0x08000000,
            )
        except Exception as e:
            messagebox.showwarning("打开独立小窗失败", str(e), parent=self)

    def _build_page_voice(self):
        page, col = self._page("语音输入", "按下快捷键说话或内录电脑声音，识别结果直接上屏 · 独立工具小窗即点即用")
        v = self.voice_cfg

        # ---- Sleek Standalone External Mini-Window Launch Card (独立简约外部小窗)
        g_mini = self._group(
            col,
            "✨ 独立外部效率小窗（也可直接点击桌面悬浮胶囊上的 🎙 / ◉ 按钮一键呼出）",
            note="将实时转写双光标工作台与 48kHz 录音棚独立为轻量外部小窗，设置界面清爽不臃肿",
            pady_top=16,
        )
        r_mv = self._row(
            g_mini,
            "🎙 实时语音与电脑声音转写 · 双光标协同小窗",
            "一键转录电脑系统/指定窗口播放的网课、会议或视频声音；支持「⌨️打字光标 + ┃🎙转录光标」同时出现、边听边改",
            wrap=450,
            badge="胶囊 🎙 键",
        )
        self._btn(
            r_mv.slot,
            "🗗 弹出转写与双光标小窗",
            lambda: self._launch_mini_window("--mini-voice"),
            kind="primary",
            height=34,
        ).pack()

        r_mr = self._row(
            g_mini,
            "◉ 48kHz 专业内录录音棚 & 离线文件转文字 · 独立小窗",
            "48kHz / 16-bit 立体声无损直录、进程级指定窗口抗干扰内录、循环试听、转文字后销毁及音频文件批量转写",
            wrap=450,
            badge="胶囊 ◉ 键",
        )
        self._btn(
            r_mr.slot,
            "🗗 弹出 48kHz 录音棚小窗",
            lambda: self._launch_mini_window("--mini-recorder"),
            kind="secondary",
            height=34,
        ).pack()

        g = self._group(col, "语音输入与音源")
        r = self._row(g, "启用语音输入", "开启后可在任意输入框中用快捷键开始听写")
        self.voice_sw = self._switch(r.slot, bool(v.get("enabled", True)),
                                     lambda: self._set_voice("enabled", bool(self.voice_sw.get())))
        self.voice_sw.pack()
        r = self._row(g, "默认录音音源", "清晰三选一：选择麦克风自动切换为「单光标跟随」；选择电脑声音自动切换为「双光标协同」", stacked=True)
        self.voice_src_seg = self._icon_choice(
            r.slot,
            [
                ("仅麦克风", "mic", "mic"),
                ("仅电脑声音 (WASAPI)", "system", "speaker"),
                ("麦克风+电脑全部声音", "both", "headset"),
            ],
            v.get("audio_source", "mic"),
            self._on_default_audio_source_change,
        )
        self.voice_src_seg.pack(anchor="w")
        r = self._row(g, "麦克风听写快捷键", "按一次开始录音，再按一次结束（连续模式下停顿即刻切块转写上屏）")
        self._chip(r.slot, str(v.get("hotkey", "Ctrl+Alt+V")), color="text", size=13).pack()
        r = self._row(g, "系统内录专属快捷键", "无需切换默认音源，按下即刻内录电脑正在播放的声音并转成文字上屏", badge="会议/网课神器")
        self._chip(r.slot, str(v.get("loopback_hotkey", "Ctrl+Alt+B")), color="text", size=13).pack()
        r = self._row(g, "连续长对话模式（10~20 分钟不间断）", "开启后支持长篇口述，每逢自然停顿即刻后台转写上屏且不中断麦克风，再按快捷键结束", badge="流式上屏")
        self.voice_cont_sw = self._switch(r.slot, bool(v.get("continuous_mode", True)),
                                          lambda: self._set_voice("continuous_mode", bool(self.voice_cont_sw.get())))
        self.voice_cont_sw.pack()
        r = self._row(g, "口误智能自纠与语义同音纠错", "根据整句语义自动纠正谐音错别字（如重新软件→重启软件、超声窝→超声波）；智能过滤语气词与口误", badge="语义自纠")
        self.voice_corr_sw = self._switch(r.slot, bool(v.get("smart_correction", True)),
                                          lambda: self._set_voice("smart_correction", bool(self.voice_corr_sw.get())))
        self.voice_corr_sw.pack()
        r = self._row(g, "个人词库热词注入", "自动读取「词库」页中的专有名词与人名（user_custom.tsv）注入 Whisper 提示词，大幅提升专业词汇识别率")
        self._chip(r.slot, "已自动联动 user_custom.tsv", color="success", mono=False, size=12).pack()
        r_rules = self._row(
            g,
            "自定义错词替换规则与冲突检测",
            "格式：错词=>正词; 别名1,别名2=>正词（采用占位符最长优先匹配，杜绝短词破坏长词与循环误替换）",
            stacked=True,
            badge="最长匹配·防冲突",
        )
        rules_box = ctk.CTkFrame(r_rules.slot, fg_color="transparent")
        rules_box.pack(fill="x")
        self.voice_replace_rules_entry = self._entry(
            rules_box, 440, "如：鹤雨,河宇=>鹤羽; 云计算平台=>云平台"
        )
        cur_rules = str(v.get("replace_rules", "")).strip()
        if cur_rules:
            self.voice_replace_rules_entry.insert(0, cur_rules)
        self.voice_replace_rules_entry.pack(side="left", padx=(0, 8))
        self.voice_replace_rules_entry.bind("<Return>", lambda e: self._save_voice_replace_rules())
        self._btn(rules_box, "保存规则", self._save_voice_replace_rules, kind="primary", height=32).pack(side="left", padx=(0, 6))
        self._btn(rules_box, "🔍 冲突体检", self._check_voice_replace_rules_ui, height=32).pack(side="left")
        self.voice_rules_status_lbl = self._label(r_rules.slot, "", 12, color="muted")
        self.voice_rules_status_lbl.pack(anchor="w", pady=(4, 0))
        self._update_voice_rules_status_label(cur_rules)
        r = self._row(g, "光标协同模式（边听边打字编辑）", "选择麦克风转写时自动切换为「单光标跟随」；选择电脑转写时自动切换为「双光标协同」（也可手动切换）", stacked=True, badge="音源智能联动")
        self.voice_cursor_bar = self._icon_choice(
            r.slot,
            [
                ("双光标分头行动（Ctrl+Alt+左键定语音位置）", "independent", "dual_cursor"),
                ("单光标跟随（鼠标点哪就转录在哪）", "follow", "target"),
            ],
            v.get("cursor_mode", "independent"),
            lambda k: self._set_voice("cursor_mode", k),
        )
        self.voice_cursor_bar.pack(anchor="w")
        r = self._row(g, "识别语言与跨语种同传翻译", "默认「自动检测 (原声)」听到什么语种就转写什么语种；若选择固定目标语言，则将听到的多语种混合音频实时翻译并输出为所选语言", stacked=True, badge="多语种同传")
        cur_tr = str(v.get("translate_target", "off")).lower()
        cur_lg = str(v.get("language", "auto")).lower()
        init_lang_mode = cur_tr if cur_tr in ("zh", "en", "it", "es") else ("follow" if cur_lg == "follow" else "auto")
        self._icon_choice(
            r.slot,
            [
                ("自动检测 (原声转写)", "auto", "globe"),
                ("实时译为中文", "zh", "flag_cn"),
                ("实时译为英语", "en", "flag_gb"),
                ("实时译为意语", "it", "flag_it"),
                ("实时译为西语", "es", "flag_es"),
                ("跟随输入法模式", "follow", "target"),
            ],
            init_lang_mode,
            self._on_voice_lang_or_translate_change,
        ).pack(anchor="w")

        g = self._group(col, "识别模型", note=f"模型目录  {MODELS_DIR}")
        self.voice_model_var = tk.StringVar(value=v.get("model", VOICE_DEFAULTS["model"]))
        self._model_rows = {}
        for mid, name, note, rec in VOICE_MODELS:
            r = self._row(g, name, f"{note}  ·  ggml-{mid}.bin", wrap=420, badge="推荐" if rec else None)
            status = self._label(r.slot, "", 12, "bold")
            status.pack(side="left", padx=(0, 12))
            dl = self._btn(r.slot, "下载模型", lambda m=mid: self._download_model(m), height=30)
            radio = ctk.CTkRadioButton(r.slot, text="使用", variable=self.voice_model_var, value=mid,
                                       font=self.F(13), text_color=C("text"), width=0,
                                       radiobutton_width=20, radiobutton_height=20, border_width_unchecked=2,
                                       border_width_checked=6, fg_color=C("accent"), hover_color=C("accent_hover"),
                                       border_color=C("border_strong"),
                                       command=lambda m=mid: self._set_voice("model", m))
            radio.pack(side="right")
            self._model_rows[mid] = (status, dl, radio)
        self._refresh_model_status()

        g = self._group(col, "性能与行为")
        r = self._row(g, "GPU 加速", "自动：有可用显卡时使用 GPU，否则使用 CPU")
        self._choice(r.slot, [("自动", "auto"), ("开启", "on"), ("关闭", "off")], v.get("use_gpu", "auto"),
                     lambda k: self._set_voice("use_gpu", k)).pack()
        r = self._row(g, "静音自动分句时长", "连续模式下检测到该时长静音即自动切块转写上屏")
        sil = [800, 1200, 2000, 3000]
        cur_sil = int(v.get("auto_stop_silence_ms", 1200))
        if cur_sil not in sil:
            sil = sorted(sil + [cur_sil])
        self._choice(r.slot, [(f"{s / 1000:g} 秒", s) for s in sil], cur_sil,
                     lambda k: self._set_voice("auto_stop_silence_ms", int(k))).pack()
        r = self._row(g, "单次最长录音上限", "连续长对话模式默认支持 20 分钟（1200 秒）不间断听写")
        mx_opts = [("1 分钟", 60), ("5 分钟", 300), ("20 分钟", 1200), ("30 分钟", 1800)]
        cur_mx = int(v.get("max_record_s", 1200))
        if cur_mx not in [m for _, m in mx_opts]:
            cur_mx = 1200
        self._choice(r.slot, mx_opts, cur_mx,
                     lambda k: self._set_voice("max_record_s", int(k))).pack()
        r = self._row(g, "输出简体中文", "将识别出的繁体字自动转换为简体")
        self.voice_simp_sw = self._switch(r.slot, bool(v.get("simplified_chinese", True)),
                                          lambda: self._set_voice("simplified_chinese", bool(self.voice_simp_sw.get())))
        self.voice_simp_sw.pack()
        r = self._row(
            g,
            "空闲自动释放模型内存",
            "超过设定时长未使用语音时自动释放 500MB~1.5GB 内存/显存；下次按键 0ms 启动录音并后台秒级重载，绝不漏字",
            badge="智能省内存",
        )
        idle_opts = [("15 分钟", 15), ("30 分钟 (推荐)", 30), ("60 分钟", 60), ("永不卸载", 0)]
        cur_idle = int(v.get("idle_unload_mins", 30))
        if cur_idle not in [m for _, m in idle_opts]:
            cur_idle = 30
        self._choice(
            r.slot,
            idle_opts,
            cur_idle,
            lambda k: self._set_voice("idle_unload_mins", int(k)),
        ).pack()

        # privacy card
        pc = self._card(col, fg="success_soft", border="success_soft")
        pc.pack(fill="x", pady=(24, 0))
        top = ctk.CTkFrame(pc, fg_color="transparent")
        top.pack(fill="x", padx=ROW_PX, pady=(16, 6))
        if self.icon_family:
            ctk.CTkLabel(top, text="\uea18", font=self.F(18, icon=True), text_color=C("success"), width=24, height=0).pack(side="left", padx=(0, 10))
        self._label(top, "100% 离线 · 隐私优先", 15, "bold", color="success").pack(side="left")
        for line in (
            "语音识别与专业录音完全在本机运行，音频永远不会离开这台电脑。",
            "快捷键听写音频仅保存在内存中；专业录音机支持「转文字后即刻销毁原音频」与「全零覆写就地销毁」。",
            "识别引擎为开源 whisper.cpp（MIT 许可），模型为 OpenAI Whisper 开源权重（MIT 许可）。",
            "模型只从 Hugging Face / GitHub 官方地址下载，下载完成后无需任何网络连接。",
        ):
            self._label(pc, f"•  {line}", 13, color="text_2", wraplength=740).pack(anchor="w", padx=ROW_PX + 2, pady=2)
        ctk.CTkFrame(pc, fg_color="transparent", height=10).pack()
        return page

    # =========================================================================
    # STANDALONE MINI-WINDOW 1: 🎙 实时声音转写（紧凑下拉收纳 · 高清矢量国旗图标 · 悬停快捷键提示）
    # =========================================================================
    def _build_mini_voice_shell(self):
        v = self.voice_cfg
        if int(v.get("target_pid", 0)) != 0:
            self._set_voice("target_pid", 0)
        if int(v.get("max_record_s", 1200)) < 3600:
            self._set_voice("max_record_s", 7200)
        self._studio_src_var = tk.StringVar(value="system")
        self._mv_phase = "idle"
        init_src = str(v.get("audio_source", "system")).strip().lower()
        if init_src not in ("mic", "system", "both"):
            init_src = "system"
            self.voice_cfg["audio_source"] = "system"
        # Auto-sync cursor mode with initial audio source (mic -> follow/单光标, system/both -> independent/双光标)
        want_cm = "follow" if init_src == "mic" else "independent"
        if str(v.get("cursor_mode", "")) != want_cm:
            self._set_voice("cursor_mode", want_cm)
        self._mv_active_src = init_src
        self._mv_anim_tick = 0
        self._mv_last_btn_sig = None
        self._mv_last_lock_sig = None
        self._mv_last_status_sig = None

        root = ctk.CTkFrame(self, fg_color=C("bg"))
        root.pack(fill="x", expand=False, padx=12, pady=(8, 8))
        self._mv_root = root

        # 1. Sleek Compact Top Bar
        hi = ctk.CTkFrame(root, fg_color="transparent")
        hi.pack(fill="x", pady=(0, 6))

        is_dark = ctk.get_appearance_mode() == "Dark"
        self.sb_theme_btn = ctk.CTkButton(
            hi,
            text="",
            image=self._ui_icon("moon" if is_dark else "sun", (15, 15)),
            command=self._toggle_mini_theme,
            height=26,
            width=30,
            corner_radius=8,
            fg_color=C("surface"),
            hover_color=C("surface_alt"),
            border_width=1,
            border_color=C("border_strong"),
        )
        self.sb_theme_btn.pack(side="right")
        self._attach_tooltip(self.sb_theme_btn, "切换浅色 / 深色界面主题")

        pin_box = ctk.CTkFrame(hi, fg_color="transparent")
        pin_box.pack(side="right", padx=(0, 8))
        self.mini_pin_sw = self._switch(pin_box, True, self._toggle_mini_topmost)
        self.mini_pin_sw.pack(side="left")
        pin_lbl = self._label(pin_box, "置顶", 12, color="text_2")
        pin_lbl.pack(side="left", padx=(4, 0))
        self._attach_tooltip(pin_lbl, "保持此悬浮转写小窗在最前端显示")
        self._toggle_mini_topmost()

        self.mv_badge_lbl = ctk.CTkLabel(
            hi,
            text="",
            image=self._ui_icon("mic_badge", (15, 15)),
            width=26,
            height=26,
            corner_radius=8,
            fg_color=C("accent"),
        )
        self.mv_badge_lbl.pack(side="left")
        self._label(hi, "实时声音转写", 14, "bold").pack(side="left", padx=(6, 8))

        # 10-second Auto-Lock Target Window Pill Button (Click to Lock/Unlock immediately)
        self.mv_lock_btn = ctk.CTkButton(
            hi,
            text="目标: 跟随窗口",
            image=self._ui_icon("target", (14, 14)),
            compound="left",
            command=self._mini_toggle_target_lock,
            height=26,
            width=126,
            corner_radius=8,
            fg_color=C("surface_alt"),
            hover_color=C("nav_hover"),
            border_width=1,
            border_color=C("border_strong"),
            text_color=C("text_2"),
            font=self.F(11, "bold"),
        )
        self.mv_lock_btn.pack(side="left")
        self._attach_tooltip(
            self.mv_lock_btn,
            "智能 10 秒防乱跳锁窗：\n"
            "• 开始转写后点击目标软件，满 10 秒自动锁定输出到该窗口\n"
            "• 点击此按钮可随时手动立即锁定或解锁目标窗口",
        )

        # 2. Ultra-Compact Control Card (Zero Wasted Space)
        ctrl = self._card(root)
        ctrl.pack(fill="x", expand=False)

        # Row 1: Three Short, Crisp Audio Source Buttons on One Row (Shortcuts in Hover Tooltips)
        btn_row = ctk.CTkFrame(ctrl, fg_color="transparent")
        btn_row.pack(fill="x", padx=10, pady=(9, 6))
        for col_i in range(3):
            btn_row.grid_columnconfigure(col_i, weight=1, uniform="mvsrc")

        mic_sel = (init_src == "mic")
        sys_sel = (init_src == "system")
        both_sel = (init_src == "both")

        self.mv_mic_btn = ctk.CTkButton(
            btn_row,
            text="仅麦克风",
            image=self._ui_icon("mic", (15, 15)),
            compound="left",
            command=self._mini_toggle_mic_voice,
            height=32,
            corner_radius=8,
            fg_color=C("accent") if mic_sel else C("surface_alt"),
            hover_color=C("accent_hover") if mic_sel else C("nav_hover"),
            border_width=0 if mic_sel else 1,
            border_color=C("border_strong"),
            text_color=C("on_accent") if mic_sel else C("text"),
            font=self.F(12, "bold"),
        )
        self.mv_mic_btn.grid(row=0, column=0, sticky="ew", padx=(0, 4))
        self._attach_tooltip(
            self.mv_mic_btn,
            "仅录制麦克风人声（自动切换为单光标跟随，停顿自动切块流式上屏）\n快捷键：Ctrl+Alt+V（中文模式输入 oiu）",
        )

        self.mv_sys_btn = ctk.CTkButton(
            btn_row,
            text="仅电脑声",
            image=self._ui_icon("speaker_on" if sys_sel else "speaker", (15, 15)),
            compound="left",
            command=self._mini_hero_system_action,
            height=32,
            corner_radius=8,
            fg_color=C("accent") if sys_sel else C("surface_alt"),
            hover_color=C("accent_hover") if sys_sel else C("nav_hover"),
            border_width=0 if sys_sel else 1,
            border_color=C("border_strong"),
            text_color=C("on_accent") if sys_sel else C("text"),
            font=self.F(12, "bold"),
        )
        self.mv_sys_btn.grid(row=0, column=1, sticky="ew", padx=4)
        self._attach_tooltip(
            self.mv_sys_btn,
            "仅内录电脑系统或指定软件声音（自动切换为双光标协同，抗麦克风环境噪音干扰）\n快捷键：Ctrl+Alt+B",
        )

        self.mv_both_btn = ctk.CTkButton(
            btn_row,
            text="麦+电脑",
            image=self._ui_icon("headset", (15, 15)),
            compound="left",
            command=self._mini_toggle_both_voice,
            height=32,
            corner_radius=8,
            fg_color=C("accent") if both_sel else C("surface_alt"),
            hover_color=C("accent_hover") if both_sel else C("nav_hover"),
            border_width=0 if both_sel else 1,
            border_color=C("border_strong"),
            text_color=C("on_accent") if both_sel else C("text"),
            font=self.F(12, "bold"),
        )
        self.mv_both_btn.grid(row=0, column=2, sticky="ew", padx=(4, 0))
        self._attach_tooltip(
            self.mv_both_btn,
            "同时混合录制麦克风与电脑播放声音（自动切换为双光标协同，适合双人会议/访谈）",
        )

        # Row 2: Single Unified Toolbar Row — [窗口下拉 + 刷新] + [语种/同传下拉菜单] + [光标模式下拉]
        bar_row = ctk.CTkFrame(ctrl, fg_color="transparent")
        bar_row.pack(fill="x", padx=10, pady=(0, 6))

        # (C) Right: Compact Cursor Mode Dropdown (Shortcuts moved to hover tooltip)
        cur_mode = str(self.voice_cfg.get("cursor_mode", want_cm))
        if cur_mode not in ("independent", "follow"):
            cur_mode = want_cm
        self.mv_cursor_seg = self._icon_dropdown(
            bar_row,
            [
                ("双光标", "independent", "dual_cursor", "双光标协同 (边听边改互不干扰)"),
                ("单光标", "follow", "target", "单光标跟随 (鼠标点哪写哪)"),
            ],
            cur_mode,
            self._on_mini_cursor_mode_changed,
            width=92,
            height=28,
            tooltip=(
                "光标协同模式：\n"
                "• 双光标协同：按 Ctrl+Alt+鼠标左键（或 Ctrl+Alt+G）定点放置独立转录光标，边听边打字互不干扰\n"
                "• 单光标跟随：鼠标点击哪里就转录在哪里"
            ),
        )
        self.mv_cursor_seg.pack(side="right", padx=(5, 0))

        # (B) Middle: Compact Language & Real-Time Translation Dropdown (with crisp Flag & Globe icons)
        cur_tr = str(v.get("translate_target", "off")).lower()
        init_mv_lang = cur_tr if cur_tr in ("zh", "en", "it", "es") else "auto"
        self._mv_lang_opts = [
            ("原声自动", "auto", "globe", "自动检测 (原声转写)"),
            ("译中文", "zh", "flag_cn", "实时同传译为中文"),
            ("译英语", "en", "flag_gb", "实时同传译为英语"),
            ("译意语", "it", "flag_it", "实时同传译为意语"),
            ("译西语", "es", "flag_es", "实时同传译为西语"),
        ]
        self.mv_lang_seg = self._icon_dropdown(
            bar_row,
            self._mv_lang_opts,
            init_mv_lang,
            self._on_voice_lang_or_translate_change,
            width=108,
            height=28,
            tooltip=(
                "语种识别与跨语种同传翻译：\n"
                "• 原声自动：听到什么语种就转写什么语种\n"
                "• 译中文 / 译英语 / 译意语 / 译西语：将听到的音频实时同传翻译为目标语言"
            ),
        )
        self.mv_lang_seg.pack(side="right", padx=(5, 0))

        # (A) Left: Compact Window Target Dropdown (with built-in window_app icon & DWM popup) + Refresh Button
        default_win_opt = "全部系统声音（不限软件）"
        self._audio_win_map = {default_win_opt: 0}
        self._studio_target_win_var = ctk.StringVar(value=default_win_opt)
        self._studio_target_pid = 0
        self.rec_app_menu = self._icon_dropdown(
            bar_row,
            [("全部系统声音", default_win_opt, "window_app", default_win_opt)],
            default_win_opt,
            lambda k: (self._studio_target_win_var.set(k), self._on_audio_target_win_selected(k)),
            width=180,
            height=28,
            tooltip="音源窗口范围：\n• 全部系统声音：内录电脑播放的所有声音\n• 选择具体软件窗口：仅内录该软件声音，屏蔽其他软件干扰",
        )
        self.rec_app_menu.pack(side="left", fill="x", expand=True)

        ref_btn = ctk.CTkButton(
            bar_row,
            text="",
            image=self._ui_icon("refresh", (14, 14)),
            command=self._refresh_audio_target_windows,
            height=28,
            width=28,
            corner_radius=8,
            fg_color=C("surface_alt"),
            hover_color=C("nav_hover"),
            border_width=1,
            border_color=C("border_strong"),
        )
        ref_btn.pack(side="left", padx=(4, 0))
        self._attach_tooltip(ref_btn, "刷新当前打开的软件窗口列表")
        self.rec_app_hint_lbl = None
        self.after(150, self._refresh_audio_target_windows)

        # Row 3: Dynamic Audio Meter & Concise Status Line
        m_row = ctk.CTkFrame(ctrl, fg_color="transparent")
        m_row.pack(fill="x", padx=10, pady=(0, 8))
        self.mv_meter_bar = ctk.CTkProgressBar(
            m_row, height=5, corner_radius=3, fg_color=C("seg_bg"), progress_color=C("success")
        )
        self.mv_meter_bar.set(0.0)
        self.mv_meter_bar.pack(fill="x", pady=(0, 4))

        m_top = ctk.CTkFrame(m_row, fg_color="transparent")
        m_top.pack(fill="x")
        self.mv_status_lbl = self._label(
            m_top,
            "三音源抗干扰 · 默认自动检测原声，选固定语种即刻同传 · 悬停按钮查看快捷键",
            11,
            color="text_2",
        )
        self.mv_status_lbl.pack(side="left")
        self.mv_db_lbl = self._label(m_top, "", 11, color="muted", mono=True)
        self.mv_db_lbl.pack(side="right")

        # Auto-fit window height tightly to content so there is zero bottom blank space
        self._fit_mini_window_height(root, default_w=516, min_w=488)

        self._mv_last_vcfg_mtime = 0.0
        self._mv_last_st_mtime = 0.0
        self._mv_cached_st = None
        self._mv_last_meter_val = 0.0
        self._mini_hidden = False
        self.protocol("WM_DELETE_WINDOW", self._on_mini_close)
        self.bind("<Map>", self._on_mini_mapped, add="+")
        self.after(180, self._mini_voice_live_poll_tick)

    def _fit_mini_window_height(self, root_frame, default_w=516, min_w=488):
        """Measure actual packed height of root_frame and lock window height with zero wasted bottom space."""
        try:
            self.update_idletasks()
            sc = max(0.5, float(self._ws()))
            req_px = root_frame.winfo_reqheight()
            exact_h = max(140, int(round(req_px / sc)) + 16)
            self.geometry(f"{default_w}x{exact_h}")
            self.minsize(min_w, exact_h)
            self.maxsize(820, exact_h + 4)
        except Exception:
            pass

    def _on_voice_lang_or_translate_change(self, key: str):
        key = str(key).strip().lower()
        if key == "auto":
            self.voice_cfg["language"] = "auto"
            self.voice_cfg["translate_target"] = "off"
        elif key == "follow":
            self.voice_cfg["language"] = "follow"
            self.voice_cfg["translate_target"] = "off"
        elif key in ("zh", "en", "it", "es"):
            self.voice_cfg["language"] = "auto"
            self.voice_cfg["translate_target"] = key
        self._save_voice_cfg()
        self._mv_last_status_sig = None

    def _on_mini_close(self):
        """Hide mini-window instead of terminating Python so next popup is <5ms with 0% idle CPU."""
        self._mini_hidden = True
        try:
            self.withdraw()
        except Exception:
            self.destroy()

    def _on_mini_mapped(self, _event=None):
        if getattr(self, "_mini_hidden", False):
            self._mini_hidden = False
            try:
                self._toggle_mini_topmost()
            except Exception:
                pass

    def _mini_toggle_target_lock(self):
        _send_voice_ipc(0x0113, "toggle_lock")

    def _on_mini_cursor_mode_changed(self, mode_key: str):
        self._set_voice("cursor_mode", mode_key)
        self._mv_last_status_sig = None

    def _toggle_mini_theme(self):
        new_key = "light" if ctk.get_appearance_mode() == "Dark" else "dark"
        self._on_appearance_change(new_key)
        self._mv_last_btn_sig = None
        self._mv_last_lock_sig = None
        self._mv_last_status_sig = None
        if hasattr(self, "sb_theme_btn"):
            self.sb_theme_btn.configure(text="", image=self._ui_icon("moon" if new_key == "dark" else "sun", (15, 15)))
        if hasattr(self, "_refresh_studio_rec_src_buttons"):
            self._refresh_studio_rec_src_buttons()

    def _toggle_mini_topmost(self):
        try:
            self.attributes("-topmost", bool(self.mini_pin_sw.get()))
        except Exception:
            pass

    def _open_full_settings(self, page="voice"):
        try:
            subprocess.Popen(
                [sys.executable, os.path.abspath(__file__), "--page", page],
                cwd=BASE_DIR,
                creationflags=0x08000000,
            )
        except Exception:
            pass

    def _on_default_audio_source_change(self, src_key: str):
        self.voice_cfg["audio_source"] = src_key
        auto_cur = "follow" if src_key == "mic" else "independent"
        self.voice_cfg["cursor_mode"] = auto_cur
        self._save_voice_cfg()
        if hasattr(self, "voice_cursor_bar"):
            self.voice_cursor_bar.set(auto_cur)
        if hasattr(self, "mv_cursor_seg"):
            self.mv_cursor_seg.set(auto_cur)

    def _mini_hero_system_action(self):
        if getattr(self, "_mv_phase", "idle") in ("recording", "recognizing") and getattr(self, "_mv_active_src", "") == "system":
            self._mini_stop_voice()
        else:
            self.voice_cfg["audio_source"] = "system"
            self.voice_cfg["cursor_mode"] = "independent"
            self._save_voice_cfg()
            if hasattr(self, "mv_cursor_seg"):
                self.mv_cursor_seg.set("independent")
            _send_voice_ipc(0x800E, "start_system")

    def _mini_toggle_system_voice(self):
        self.voice_cfg["audio_source"] = "system"
        self.voice_cfg["cursor_mode"] = "independent"
        self._save_voice_cfg()
        if hasattr(self, "mv_cursor_seg"):
            self.mv_cursor_seg.set("independent")
        _send_voice_ipc(0x800B, "toggle_system")

    def _mini_toggle_mic_voice(self):
        if getattr(self, "_mv_phase", "idle") in ("recording", "recognizing") and getattr(self, "_mv_active_src", "") == "mic":
            self._mini_stop_voice()
        else:
            self.voice_cfg["audio_source"] = "mic"
            self.voice_cfg["cursor_mode"] = "follow"
            self._save_voice_cfg()
            if hasattr(self, "mv_cursor_seg"):
                self.mv_cursor_seg.set("follow")
            _send_voice_ipc(0x800D, "start_mic")

    def _mini_toggle_both_voice(self):
        if getattr(self, "_mv_phase", "idle") in ("recording", "recognizing") and getattr(self, "_mv_active_src", "") == "both":
            self._mini_stop_voice()
        else:
            self.voice_cfg["audio_source"] = "both"
            self.voice_cfg["cursor_mode"] = "independent"
            self._save_voice_cfg()
            if hasattr(self, "mv_cursor_seg"):
                self.mv_cursor_seg.set("independent")
            _send_voice_ipc(0x8010, "start_both")

    def _mini_stop_voice(self):
        _send_voice_ipc(0x800C, "stop")
        if getattr(self, "_sample_tts_proc", None) is not None:
            try:
                self._sample_tts_proc.terminate()
            except Exception:
                pass
            self._sample_tts_proc = None

    def _mini_voice_live_poll_tick(self):
        if getattr(self, "_mini_hidden", False):
            self.after(650, self._mini_voice_live_poll_tick)
            return
        next_ms = 420
        try:
            vcfg_p = os.path.join(DATA_DIR, VOICE_CONFIG_NAME)
            try:
                vm = os.path.getmtime(vcfg_p)
            except Exception:
                vm = 0.0
            if vm != getattr(self, "_mv_last_vcfg_mtime", 0.0):
                self._mv_last_vcfg_mtime = vm
                vcfg = _read_json(vcfg_p, None)
                if isinstance(vcfg, dict):
                    if hasattr(self, "mv_cursor_seg"):
                        cm = str(vcfg.get("cursor_mode", "independent"))
                        self.voice_cfg["cursor_mode"] = cm
                        if self.mv_cursor_seg.get() != cm:
                            self.mv_cursor_seg.set(cm)
                    if hasattr(self, "mv_lang_seg"):
                        tr = str(vcfg.get("translate_target", "off")).lower()
                        self.voice_cfg["translate_target"] = tr
                        want_k = tr if tr in ("zh", "en", "it", "es") else "auto"
                        if self.mv_lang_seg.get() != want_k:
                            self.mv_lang_seg.set(want_k)

            st_p = os.path.join(DATA_DIR, "voice_live_status.json")
            try:
                sm = os.path.getmtime(st_p)
            except Exception:
                sm = 0.0
            if sm != getattr(self, "_mv_last_st_mtime", 0.0):
                self._mv_last_st_mtime = sm
                self._mv_cached_st = _read_json(st_p, None)
            st = getattr(self, "_mv_cached_st", None)
            if isinstance(st, dict):
                raw_ts = float(st.get("ts", 0))
                if raw_ts > 1e11:
                    raw_ts /= 1000.0
                if (time.time() - raw_ts) < 6.0:
                    phase = st.get("phase", "idle")
                    if phase in ("recording", "recognizing"):
                        next_ms = 220
                    src = st.get("source", "system")
                    lvl = float(st.get("level", 0.0))
                    chunks = int(st.get("chunks", st.get("committed_chunks", 0)))
                    last_txt = str(st.get("last_text", "") or "").strip()
                    t_title = str(st.get("target_title", "") or "").strip()
                    t_locked = bool(st.get("target_locked", False))
                    lock_rem = int(st.get("lock_rem_s", 10))
                    self._mv_phase = phase
                    self._mv_active_src = src
                    self._mv_anim_tick = (getattr(self, "_mv_anim_tick", 0) + 1) % 4

                    # 1. Update 10-second Target Window Lock pill only when changed
                    short_win = t_title if len(t_title) <= 9 else (t_title[:8] + "…")
                    if t_locked and short_win:
                        lock_txt = f"已锁: {short_win}"
                        lock_ic = "lock"
                        lock_fg = C("success_soft")
                        lock_tc = C("success")
                    elif phase in ("recording", "recognizing") and short_win:
                        lock_txt = f"{lock_rem}s锁: {short_win}"
                        lock_ic = "hourglass"
                        lock_fg = C("accent_soft")
                        lock_tc = C("accent")
                    elif short_win:
                        lock_txt = f"目标: {short_win}"
                        lock_ic = "target"
                        lock_fg = C("surface_alt")
                        lock_tc = C("text_2")
                    else:
                        lock_txt = "目标: 跟随窗口"
                        lock_ic = "target"
                        lock_fg = C("surface_alt")
                        lock_tc = C("text_2")

                    lock_sig = (lock_txt, lock_ic, lock_fg, lock_tc)
                    if getattr(self, "_mv_last_lock_sig", None) != lock_sig and hasattr(self, "mv_lock_btn"):
                        self._mv_last_lock_sig = lock_sig
                        self.mv_lock_btn.configure(
                            text=lock_txt,
                            image=self._ui_icon(lock_ic, (14, 14)),
                            fg_color=lock_fg,
                            text_color=lock_tc,
                        )

                    # 2. Update the 3 Action Pill Buttons only when state/wave changes
                    if phase in ("recording", "recognizing"):
                        wave = ("((●))", "( ● )", "((●))", "((●))")[self._mv_anim_tick]
                        btn_sig = (phase, src, wave)
                        if getattr(self, "_mv_last_btn_sig", None) != btn_sig:
                            self._mv_last_btn_sig = btn_sig
                            if hasattr(self, "mv_badge_lbl"):
                                self.mv_badge_lbl.configure(fg_color=C("danger"))

                            # Button 1: Mic Only
                            if src == "mic":
                                self.mv_mic_btn.configure(
                                    text=f"{wave} 麦克风中",
                                    image=self._ui_icon("stop_wave", (15, 15)),
                                    fg_color=C("danger"),
                                    hover_color="#b91c1c",
                                    text_color="#ffffff",
                                )
                            else:
                                self.mv_mic_btn.configure(
                                    text="切为麦克风",
                                    image=self._ui_icon("mic", (15, 15)),
                                    fg_color=C("surface_alt"),
                                    hover_color=C("nav_hover"),
                                    text_color=C("text"),
                                )

                            # Button 2: System Only
                            if src == "system":
                                self.mv_sys_btn.configure(
                                    text=f"{wave} 电脑声中",
                                    image=self._ui_icon("stop_wave", (15, 15)),
                                    fg_color=C("danger"),
                                    hover_color="#b91c1c",
                                    text_color="#ffffff",
                                )
                            else:
                                self.mv_sys_btn.configure(
                                    text="切为电脑声",
                                    image=self._ui_icon("speaker", (15, 15)),
                                    fg_color=C("surface_alt"),
                                    hover_color=C("nav_hover"),
                                    text_color=C("text"),
                                )

                            # Button 3: Mic + System Both
                            if src == "both":
                                self.mv_both_btn.configure(
                                    text=f"{wave} 麦+电脑中",
                                    image=self._ui_icon("stop_wave", (15, 15)),
                                    fg_color=C("danger"),
                                    hover_color="#b91c1c",
                                    text_color="#ffffff",
                                )
                            else:
                                self.mv_both_btn.configure(
                                    text="切为麦+电脑",
                                    image=self._ui_icon("headset", (15, 15)),
                                    fg_color=C("surface_alt"),
                                    hover_color=C("nav_hover"),
                                    text_color=C("text"),
                                )
                        mval = round(max(0.04, min(1.0, lvl)), 3)
                        if getattr(self, "_mv_last_meter_val", -1.0) != mval:
                            self._mv_last_meter_val = mval
                            self.mv_meter_bar.set(mval)
                        if last_txt:
                            short_t = last_txt if len(last_txt) <= 24 else ("…" + last_txt[-23:])
                            st_txt = f"已写出 {chunks} 句 · “{short_t}”"
                            st_col = C("success")
                        elif t_locked and short_win:
                            st_txt = f"已锁定输出至「{short_win}」· 可自由切换其他软件操作"
                            st_col = C("success")
                        else:
                            src_lbl = "仅电脑声音(抗麦克风干扰)" if src == "system" else ("麦+电脑混合声" if src == "both" else "仅麦克风")
                            st_txt = f"正在聆听【{src_lbl}】… 满 10 秒自动锁定目标窗口防乱跳"
                            st_col = C("accent")
                    else:
                        cfg_src = str(self.voice_cfg.get("audio_source", "system")).strip().lower()
                        if cfg_src not in ("mic", "system", "both"):
                            cfg_src = "system"
                        btn_sig = ("idle", cfg_src, "")
                        if getattr(self, "_mv_last_btn_sig", None) != btn_sig:
                            self._mv_last_btn_sig = btn_sig
                            if hasattr(self, "mv_badge_lbl"):
                                self.mv_badge_lbl.configure(fg_color=C("accent"))
                            mic_sel = (cfg_src == "mic")
                            sys_sel = (cfg_src == "system")
                            both_sel = (cfg_src == "both")
                            self.mv_mic_btn.configure(
                                text="仅麦克风",
                                image=self._ui_icon("mic", (15, 15)),
                                fg_color=C("accent") if mic_sel else C("surface_alt"),
                                hover_color=C("accent_hover") if mic_sel else C("nav_hover"),
                                border_width=0 if mic_sel else 1,
                                text_color=C("on_accent") if mic_sel else C("text"),
                            )
                            self.mv_sys_btn.configure(
                                text="仅电脑声",
                                image=self._ui_icon("speaker_on" if sys_sel else "speaker", (15, 15)),
                                fg_color=C("accent") if sys_sel else C("surface_alt"),
                                hover_color=C("accent_hover") if sys_sel else C("nav_hover"),
                                border_width=0 if sys_sel else 1,
                                text_color=C("on_accent") if sys_sel else C("text"),
                            )
                            self.mv_both_btn.configure(
                                text="麦+电脑",
                                image=self._ui_icon("headset", (15, 15)),
                                fg_color=C("accent") if both_sel else C("surface_alt"),
                                hover_color=C("accent_hover") if both_sel else C("nav_hover"),
                                border_width=0 if both_sel else 1,
                                text_color=C("on_accent") if both_sel else C("text"),
                            )
                        if getattr(self, "_mv_last_meter_val", -1.0) != 0.0:
                            self._mv_last_meter_val = 0.0
                            self.mv_meter_bar.set(0.0)
                        if chunks > 0 and last_txt:
                            short_t = last_txt if len(last_txt) <= 24 else ("…" + last_txt[-23:])
                            st_txt = f"已暂停（共写出 {chunks} 句 · 最近：“{short_t}”）"
                            st_col = C("text_2")
                        else:
                            tr = str(self.voice_cfg.get("translate_target", "off")).lower()
                            tr_map = {"zh": "同传译中文", "en": "同传译英语", "it": "同传译意语", "es": "同传译西语"}
                            tr_tag = tr_map.get(tr, "自动检测原声")
                            st_txt = f"三音源抗干扰 · 当前：{tr_tag} · 悬停按钮查看快捷键"
                            st_col = C("text_2")

                    st_sig = (st_txt, st_col)
                    if getattr(self, "_mv_last_status_sig", None) != st_sig:
                        self._mv_last_status_sig = st_sig
                        self.mv_status_lbl.configure(text=st_txt, text_color=st_col)
        except Exception:
            pass
        self.after(next_ms, self._mini_voice_live_poll_tick)

    # =========================================================================
    # STANDALONE MINI-WINDOW 2: ◉ 48kHz 录音棚（极简紧凑单行下拉设计）
    # =========================================================================
    def _build_mini_recorder_shell(self):
        v = self.voice_cfg
        if int(v.get("target_pid", 0)) != 0:
            self._set_voice("target_pid", 0)

        root = ctk.CTkFrame(self, fg_color=C("bg"))
        root.pack(fill="x", expand=False, padx=12, pady=(8, 8))

        # 1. Minimal Top Bar
        hi = ctk.CTkFrame(root, fg_color="transparent")
        hi.pack(fill="x", pady=(0, 6))

        is_dark = ctk.get_appearance_mode() == "Dark"
        self.sb_theme_btn = ctk.CTkButton(
            hi,
            text="",
            image=self._ui_icon("moon" if is_dark else "sun", (15, 15)),
            command=self._toggle_mini_theme,
            height=26,
            width=30,
            corner_radius=8,
            fg_color=C("surface"),
            hover_color=C("surface_alt"),
            border_width=1,
            border_color=C("border_strong"),
        )
        self.sb_theme_btn.pack(side="right")
        self._attach_tooltip(self.sb_theme_btn, "切换浅色 / 深色界面主题")

        pin_box = ctk.CTkFrame(hi, fg_color="transparent")
        pin_box.pack(side="right", padx=(0, 8))
        self.mini_pin_sw = self._switch(pin_box, True, self._toggle_mini_topmost)
        self.mini_pin_sw.pack(side="left")
        self._label(pin_box, "置顶", 12, color="text_2").pack(side="left", padx=(4, 0))
        self._toggle_mini_topmost()

        ctk.CTkLabel(
            hi,
            text="",
            image=self._ui_icon("rec_badge", (15, 15)),
            width=26,
            height=26,
            corner_radius=8,
            fg_color=C("accent"),
        ).pack(side="left")
        self._label(hi, "48kHz 录音棚 & 音频转文字", 14, "bold").pack(side="left", padx=(7, 0))

        # 2. Clean Compact Studio Card
        g_rec = self._card(root)
        g_rec.pack(fill="x", expand=False)

        init_rec_src = str(v.get("audio_source", "mic")).strip().lower()
        if init_rec_src not in ("mic", "system", "both"):
            init_rec_src = "mic"
        self._studio_src_var = tk.StringVar(value=init_rec_src)

        # Row 1: Three Explicit Audio Source Buttons (Matching 实时声音转写 100%)
        src_row = ctk.CTkFrame(g_rec, fg_color="transparent")
        src_row.pack(fill="x", padx=12, pady=(10, 6))
        for col_i in range(3):
            src_row.grid_columnconfigure(col_i, weight=1, uniform="recsrc")

        self.rec_mic_btn = ctk.CTkButton(
            src_row,
            text="仅麦克风",
            image=self._ui_icon("mic", (15, 15)),
            compound="left",
            command=lambda: self._set_studio_rec_source("mic"),
            height=32,
            corner_radius=8,
            border_color=C("border_strong"),
            font=self.F(12, "bold"),
        )
        self.rec_mic_btn.grid(row=0, column=0, sticky="ew", padx=(0, 4))
        self._attach_tooltip(self.rec_mic_btn, "仅录制麦克风人声（48.0 kHz / 16-bit 无损立体声 + 人声母带降噪）")

        self.rec_sys_btn = ctk.CTkButton(
            src_row,
            text="仅电脑声",
            image=self._ui_icon("speaker", (15, 15)),
            compound="left",
            command=lambda: self._set_studio_rec_source("system"),
            height=32,
            corner_radius=8,
            border_color=C("border_strong"),
            font=self.F(12, "bold"),
        )
        self.rec_sys_btn.grid(row=0, column=1, sticky="ew", padx=4)
        self._attach_tooltip(self.rec_sys_btn, "仅内录电脑系统或指定软件声音（WASAPI 无损内录，彻底屏蔽麦克风环境杂音）")

        self.rec_both_btn = ctk.CTkButton(
            src_row,
            text="麦+电脑",
            image=self._ui_icon("headset", (15, 15)),
            compound="left",
            command=lambda: self._set_studio_rec_source("both"),
            height=32,
            corner_radius=8,
            border_color=C("border_strong"),
            font=self.F(12, "bold"),
        )
        self.rec_both_btn.grid(row=0, column=2, sticky="ew", padx=(4, 0))
        self._attach_tooltip(self.rec_both_btn, "同时混合录制麦克风人声 + 电脑播放声音（适合会议对谈、访谈、双人播客录制）")

        # Row 2: Window Target Dropdown + Refresh
        r_top = ctk.CTkFrame(g_rec, fg_color="transparent")
        r_top.pack(fill="x", padx=12, pady=(0, 8))

        self._audio_win_map = _list_audio_target_windows()
        default_win_opt = next(iter(self._audio_win_map.keys()))
        self._studio_target_win_var = ctk.StringVar(value=default_win_opt)
        self._studio_target_pid = 0
        ref_btn2 = ctk.CTkButton(
            r_top,
            text="",
            image=self._ui_icon("refresh", (14, 14)),
            command=self._refresh_audio_target_windows,
            height=28,
            width=30,
            corner_radius=8,
            fg_color=C("surface_alt"),
            hover_color=C("nav_hover"),
            border_width=1,
            border_color=C("border_strong"),
        )
        ref_btn2.pack(side="right", padx=(5, 0))
        self._attach_tooltip(ref_btn2, "刷新当前打开的软件窗口列表")

        init_dd_opts = [
            (k if len(k) <= 22 else (k[:21] + "…"), k, "window_app", k if len(k) <= 42 else (k[:41] + "…"))
            for k in self._audio_win_map.keys()
        ]
        self.rec_app_menu = self._icon_dropdown(
            r_top,
            init_dd_opts,
            default_win_opt,
            lambda k: (self._studio_target_win_var.set(k), self._on_audio_target_win_selected(k)),
            width=260,
            height=28,
            tooltip="音源窗口范围：\n• 全部系统声音：内录电脑播放的所有声音\n• 选择具体软件窗口：仅内录该软件声音，屏蔽其他软件干扰",
        )
        self.rec_app_menu.pack(side="left", fill="x", expand=True)
        self.rec_app_hint_lbl = None

        # Keep studio-grade DSP enabled automatically without cluttering the UI
        self.rec_denoise_sw = tk.BooleanVar(value=True)
        self.rec_focus_sw = tk.BooleanVar(value=True)
        self.rec_loop_sw = tk.BooleanVar(value=False)

        m_frame = ctk.CTkFrame(g_rec, fg_color="transparent")
        m_frame.pack(fill="x", padx=12, pady=(0, 6))
        self.rec_meter_bar = ctk.CTkProgressBar(
            m_frame, height=6, corner_radius=3, fg_color=C("seg_bg"), progress_color=C("success")
        )
        self.rec_meter_bar.set(0.0)
        self.rec_meter_bar.pack(fill="x", pady=(0, 5))
        top_m = ctk.CTkFrame(m_frame, fg_color="transparent")
        top_m.pack(fill="x")
        self.rec_timer_lbl = self._label(top_m, "就绪 · 48.0 kHz 无损立体声", 11, "bold", mono=True)
        self.rec_timer_lbl.pack(side="left")
        self.rec_db_lbl = self._label(top_m, "峰值 -∞ dB", 11, color="muted", mono=True)
        self.rec_db_lbl.pack(side="right")

        act_bar = ctk.CTkFrame(g_rec, fg_color="transparent")
        act_bar.pack(fill="x", padx=12, pady=(0, 6))
        self.rec_start_btn = self._btn(
            act_bar, "开始录制", self._studio_rec_toggle, kind="primary", height=34,
            image=self._ui_icon("rec_badge", (15, 15)), compound="left",
        )
        self.rec_start_btn.pack(side="left", fill="x", expand=True)
        self._attach_tooltip(self.rec_start_btn, "开始录制 48kHz 无损 WAV 音频；录制完成后若不满意，可点击「重新录制」直接重录")

        self.rec_play_btn = self._btn(
            act_bar, "试听", self._studio_preview_toggle, kind="secondary", height=34,
            image=self._ui_icon("play", (14, 14)), compound="left",
        )
        self.rec_play_btn.pack(side="left", padx=(5, 0))
        self._attach_tooltip(self.rec_play_btn, "试听刚才录制的音频效果")

        self.rec_discard_btn = self._btn(
            act_bar, "放弃", self._studio_rec_discard, kind="secondary", height=34,
            image=self._ui_icon("trash", (14, 14)), compound="left",
        )
        self.rec_discard_btn.pack(side="left", padx=(5, 0))
        self._attach_tooltip(self.rec_discard_btn, "不想保存当前这段录音？点击立即删除并全零销毁该条录音，恢复待机状态")

        self.rec_save_btn = self._btn(
            act_bar, "文件夹", self._open_recordings_folder, height=34,
            image=self._ui_icon("folder_audio", (14, 14)), compound="left",
        )
        self.rec_save_btn.pack(side="left", padx=(5, 0))
        self._attach_tooltip(self.rec_save_btn, "打开录音保存文件夹并自动高亮选中最新录制的 WAV 音频文件")

        tr_btn = self._btn(
            act_bar, "转文字", self._mini_recorder_transcribe_action, height=34,
            image=self._ui_icon("save", (14, 14)), compound="left",
        )
        tr_btn.pack(side="left", padx=(5, 0))
        self._attach_tooltip(tr_btn, "将当前录制音频转文字到剪贴板，或选择本地 MP3/WAV/M4A/FLAC 文件批量转写")

        # Clickable Folder Location Bar so user always sees where audio is stored
        rec_dir = self._get_recordings_dir()
        loc_bar = ctk.CTkFrame(g_rec, fg_color="transparent")
        loc_bar.pack(fill="x", padx=12, pady=(0, 8))
        self.rec_preview_lbl = self._label(
            loc_bar,
            f"📂 录音保存位置：{rec_dir} （点击此处或「文件夹」直接打开）",
            11,
            color="text_2",
        )
        self.rec_preview_lbl.pack(side="left")
        self.rec_preview_lbl.configure(cursor="hand2")
        self.rec_preview_lbl.bind("<Button-1>", lambda _e: self._open_recordings_folder())

        self._fit_mini_window_height(root, default_w=532, min_w=500)

        self._studio_rec_proc = None
        self._studio_staging_wav = os.path.join(DATA_DIR, "tmp_studio_master.wav")
        self._studio_stop_file = os.path.join(DATA_DIR, "tmp_studio.stop")
        self._studio_status_file = os.path.join(DATA_DIR, "tmp_studio_status.json")
        self._studio_preview_playing = False
        self._studio_preview_start_ts = 0.0
        self._studio_last_duration_s = 0.0
        self._last_saved_wav = None
        self._refresh_studio_rec_src_buttons()

    def _get_recordings_dir(self) -> str:
        cand = os.path.join(BASE_DIR, "Recordings")
        try:
            os.makedirs(cand, exist_ok=True)
            test_f = os.path.join(cand, ".write_test")
            with open(test_f, "w", encoding="utf-8") as f:
                f.write("ok")
            os.remove(test_f)
            return cand
        except Exception:
            fallback = os.path.join(os.path.expanduser("~"), "Music", "HEYU_Recordings")
            os.makedirs(fallback, exist_ok=True)
            return fallback

    def _open_recordings_folder(self):
        rec_dir = self._get_recordings_dir()
        target = getattr(self, "_last_saved_wav", None)
        try:
            if target and os.path.isfile(target):
                subprocess.Popen(["explorer.exe", f'/select,"{os.path.normpath(target)}"'])
            else:
                os.startfile(rec_dir)
        except Exception:
            try:
                os.startfile(rec_dir)
            except Exception:
                pass

    def _set_studio_rec_source(self, src_key: str):
        if src_key not in ("mic", "system", "both"):
            src_key = "mic"
        self._studio_src_var.set(src_key)
        self._refresh_studio_rec_src_buttons()
        if getattr(self, "_studio_rec_proc", None) is None and hasattr(self, "rec_timer_lbl"):
            src_lbl = "仅麦克风" if src_key == "mic" else ("仅电脑声" if src_key == "system" else "麦+电脑混录")
            self.rec_timer_lbl.configure(text=f"就绪 · 当前音源：{src_lbl} · 48.0 kHz 无损立体声", text_color=C("text"))

    def _refresh_studio_rec_src_buttons(self):
        cur = self._studio_src_var.get() if hasattr(self, "_studio_src_var") else "mic"
        mic_sel = (cur == "mic")
        sys_sel = (cur == "system")
        both_sel = (cur == "both")
        if hasattr(self, "rec_mic_btn"):
            self.rec_mic_btn.configure(
                image=self._ui_icon("mic", (15, 15)),
                fg_color=C("accent") if mic_sel else C("surface_alt"),
                hover_color=C("accent_hover") if mic_sel else C("nav_hover"),
                border_width=0 if mic_sel else 1,
                text_color=C("on_accent") if mic_sel else C("text"),
            )
        if hasattr(self, "rec_sys_btn"):
            self.rec_sys_btn.configure(
                image=self._ui_icon("speaker_on" if sys_sel else "speaker", (15, 15)),
                fg_color=C("accent") if sys_sel else C("surface_alt"),
                hover_color=C("accent_hover") if sys_sel else C("nav_hover"),
                border_width=0 if sys_sel else 1,
                text_color=C("on_accent") if sys_sel else C("text"),
            )
        if hasattr(self, "rec_both_btn"):
            self.rec_both_btn.configure(
                image=self._ui_icon("headset", (15, 15)),
                fg_color=C("accent") if both_sel else C("surface_alt"),
                hover_color=C("accent_hover") if both_sel else C("nav_hover"),
                border_width=0 if both_sel else 1,
                text_color=C("on_accent") if both_sel else C("text"),
            )

    def _mini_recorder_transcribe_action(self):
        self._studio_rec_stop_internal(discard=False)
        if os.path.exists(self._studio_staging_wav) and os.path.getsize(self._studio_staging_wav) > 44:
            self._mini_transcribe_to_clipboard([self._studio_staging_wav], wipe_after=True)
        else:
            paths = filedialog.askopenfilenames(
                parent=self,
                title="选择要批量转成文字的音频文件（支持多选）",
                filetypes=[("音频文件", "*.wav;*.mp3;*.m4a;*.flac;*.ogg;*.aac;*.mp4"), ("所有文件", "*.*")],
            )
            if paths:
                self._mini_transcribe_to_clipboard(list(paths), wipe_after=False)

    def _mini_transcribe_to_clipboard(self, wav_paths, wipe_after: bool = False):
        if not os.path.exists(VOICE_EXE_PATH):
            return
        if isinstance(wav_paths, str):
            wav_paths = [wav_paths]
        if not wav_paths:
            return
        total = len(wav_paths)
        out_txt = os.path.join(DATA_DIR, "tmp_batch_transcript.txt")
        if total == 1:
            self.rec_timer_lbl.configure(text=f"⏳ 正在转写：{os.path.basename(wav_paths[0])} …", text_color=C("accent"))
        else:
            self.rec_timer_lbl.configure(text=f"⏳ [1/{total}] 正在批量转写：{os.path.basename(wav_paths[0])} …", text_color=C("accent"))

        def work():
            t0 = time.time()
            results = []
            last_rc = 0
            for idx, wp in enumerate(wav_paths):
                if total > 1:
                    self.after(
                        0,
                        lambda i=idx + 1, name=os.path.basename(wp): self.rec_timer_lbl.configure(
                            text=f"⏳ [{i}/{total}] 正在批量转写：{name} …",
                            text_color=C("accent"),
                        ),
                    )
                try:
                    if os.path.exists(out_txt):
                        os.remove(out_txt)
                except Exception:
                    pass
                proc = subprocess.run(
                    [VOICE_EXE_PATH, "--transcribe-file", wp, "--out-file", out_txt],
                    capture_output=True, text=True, encoding="utf-8", errors="replace", creationflags=0x08000000,
                )
                last_rc = proc.returncode
                txt = ""
                if os.path.exists(out_txt):
                    try:
                        txt = open(out_txt, "r", encoding="utf-8").read().strip()
                    except Exception:
                        pass
                    _vault_secure_wipe(out_txt)
                if wipe_after:
                    _vault_secure_wipe(wp)
                if txt:
                    if total > 1:
                        results.append(f"【{os.path.basename(wp)}】\n{txt}")
                    else:
                        results.append(txt)
            joined = "\n\n".join(results).strip()
            return last_rc, joined, len(results), time.time() - t0

        def done(res):
            if not res:
                self.rec_timer_lbl.configure(text="⚠ 转写失败", text_color=C("danger"))
                return
            rc, txt, ok_count, elapsed = res
            if txt:
                self.clipboard_clear()
                self.clipboard_append(txt)
                if total > 1:
                    self.rec_timer_lbl.configure(
                        text=f"✓ 已批量转写 {ok_count}/{total} 个文件并复制（共 {len(txt)} 字 · 耗时 {elapsed:.1f}s）",
                        text_color=C("success"),
                    )
                else:
                    short_t = txt if len(txt) <= 22 else (txt[:21] + "…")
                    self.rec_timer_lbl.configure(
                        text=f"✓ 已转文字并复制（{len(txt)}字 · “{short_t}”）",
                        text_color=C("success"),
                    )
            else:
                self.rec_timer_lbl.configure(text="⚠ 未检测到有效语音内容", text_color=C("danger"))

        self._run_bg(work, done)

    # ---- Per-Application Audio Target & Dual-Cursor Co-Editing Workbench -----
    VOICE_CURSOR_TOKEN = "┃🎙转录光标"

    def _refresh_audio_target_windows(self):
        self._audio_win_map = _list_audio_target_windows()
        opts = list(self._audio_win_map.keys())
        cur = self._studio_target_win_var.get()
        if cur not in self._audio_win_map:
            cur = opts[0]
            self._studio_target_win_var.set(cur)
        if hasattr(self, "rec_app_menu"):
            if hasattr(self.rec_app_menu, "set_options"):
                dd_opts = [
                    (
                        "全部系统声音" if k.startswith("全部系统声音") else (k if len(k) <= 13 else (k[:12] + "…")),
                        k,
                        "window_app",
                        k if len(k) <= 36 else (k[:35] + "…"),
                    )
                    for k in opts
                ]
                self.rec_app_menu.set_options(dd_opts, cur)
            else:
                self.rec_app_menu.configure(values=opts)
        self._on_audio_target_win_selected(cur)

    def _on_audio_target_win_selected(self, choice: str):
        pid = int(self._audio_win_map.get(choice, 0))
        self._studio_target_pid = pid
        self._set_voice("target_pid", pid)
        _send_voice_ipc(0, "")
        if pid > 0 and self._studio_src_var.get() == "mic":
            self._studio_src_var.set("system")
            if hasattr(self, "_refresh_studio_rec_src_buttons"):
                self._refresh_studio_rec_src_buttons()
        if getattr(self, "rec_app_hint_lbl", None) is not None:
            if pid > 0:
                self.rec_app_hint_lbl.configure(
                    text=f"✓ 已锁定进程树 PID={pid}",
                    text_color=C("success"),
                )
            else:
                self.rec_app_hint_lbl.configure(
                    text="当前：收录全部系统声音",
                    text_color=C("muted"),
                )

    def _create_dual_cursor_text_widget(self, parent, height=11):
        frame = ctk.CTkFrame(parent, fg_color="transparent")
        frame.pack(fill="both", expand=True, padx=4, pady=4)
        txt = tk.Text(
            frame,
            height=height,
            wrap="word",
            bd=0,
            highlightthickness=0,
            padx=14,
            pady=10,
            spacing1=4,
            spacing2=2,
            spacing3=4,
            insertwidth=3,
            undo=True,
            font=(self.ui_family, 12),
        )
        sb = ctk.CTkScrollbar(
            frame,
            command=txt.yview,
            button_color=C("border_strong"),
            button_hover_color=C("muted"),
            fg_color="transparent",
            width=12,
        )
        txt.configure(yscrollcommand=sb.set)
        sb.pack(side="right", fill="y")
        txt.pack(side="left", fill="both", expand=True)

        # Configure tags for the simultaneous 2nd cursor (`┃🎙转录光标`) and streamed text
        txt.tag_configure(
            "voice_cursor_tag",
            background="#059669",
            foreground="#ffffff",
            font=(self.ui_family, 10, "bold"),
        )
        txt.tag_configure(
            "voice_cursor_flash",
            background="#f59e0b",
            foreground="#111827",
            font=(self.ui_family, 10, "bold"),
        )
        txt.tag_configure("voice_recent", foreground="#059669")
        txt.tag_configure("heading_tag", font=(self.ui_family, 14, "bold"))

        # Hand cursor when hovering over the green transcription cursor badge
        txt.tag_bind("voice_cursor_tag", "<Enter>", lambda _e, w=txt: w.configure(cursor="hand2"))
        txt.tag_bind("voice_cursor_tag", "<Leave>", lambda _e, w=txt: w.configure(cursor="xterm"))

        # Way 1: Direct Mouse Drag-and-Drop on `┃🎙转录光标`
        txt._dragging_vc = False

        def _on_vc_press(_e, w=txt):
            w._dragging_vc = True
            return "break"

        def _on_motion(e, w=txt):
            if not getattr(w, "_dragging_vc", False):
                return
            target = w.index(f"@{e.x},{e.y}")
            self._dual_relocate_voice_cursor_in_widget(w, target, flash=False)
            return "break"

        def _on_release(e, w=txt):
            if not getattr(w, "_dragging_vc", False):
                return
            w._dragging_vc = False
            target = w.index(f"@{e.x},{e.y}")
            self._dual_relocate_voice_cursor_in_widget(w, target, flash=True)
            return "break"

        txt.tag_bind("voice_cursor_tag", "<ButtonPress-1>", _on_vc_press)
        txt.bind("<B1-Motion>", _on_motion, add="+")
        txt.bind("<ButtonRelease-1>", _on_release, add="+")

        # Way 2: Single Right-Click (`<Button-3>`) or `Alt + Left-Click` (`<Alt-Button-1>`) anywhere in text
        def _on_teleport_click(e, w=txt):
            target = w.index(f"@{e.x},{e.y}")
            self._dual_relocate_voice_cursor_in_widget(w, target, flash=True)
            return "break"

        txt.bind("<Button-3>", _on_teleport_click)
        txt.bind("<Alt-Button-1>", _on_teleport_click)

        # Update live dual-coordinate badges whenever the user types or clicks
        for ev in ("<KeyRelease>", "<ButtonRelease-1>"):
            txt.bind(ev, lambda _e, w=txt: self._dual_update_coord_badges(w), add="+")

        self._apply_single_dual_text_theme(txt)
        return txt

    def _apply_single_dual_text_theme(self, txt):
        if txt is None:
            return
        try:
            is_dark = ctk.get_appearance_mode() == "Dark"
            P = PALETTES["dark" if is_dark else "light"]
            txt.configure(
                bg=P["surface_sunken"],
                fg=P["text"],
                insertbackground="#f59e0b" if is_dark else "#1d4ed8",
                selectbackground=P["accent_soft"],
                selectforeground=P["text"],
            )
            txt.tag_configure("voice_recent", foreground="#34d399" if is_dark else "#047857")
            txt.tag_configure("heading_tag", foreground=P["accent"], font=(self.ui_family, 14, "bold"))
        except Exception:
            pass

    def _apply_dual_text_theme(self):
        for w in (getattr(self, "dual_text", None), getattr(self, "_dual_popout_text", None)):
            self._apply_single_dual_text_theme(w)

    def _dual_active_widgets(self):
        out = []
        for w in (getattr(self, "dual_text", None), getattr(self, "_dual_popout_text", None)):
            if w is not None:
                try:
                    if w.winfo_exists():
                        out.append(w)
                except Exception:
                    pass
        return out

    def _dual_init_document(self, w):
        w.delete("1.0", "end")
        intro_h = "【会议 / 课堂双光标协同速记】\n"
        intro_body = (
            "• 在此可直接用鼠标左键移动「⌨️打字光标」写大标题、换行或修改错别字；\n"
            "• 同时下方的绿色「┃🎙转录光标」会独立按照它自己的节奏持续输出语音/内录转写文字，两者互不抢位！\n"
            "• 移动转录光标：直接按住绿色「┃🎙转录光标」拖动，或在任意位置点击【鼠标右键】即可指哪打哪：\n\n"
        )
        w.insert("1.0", intro_h, ("heading_tag",))
        w.insert("end", intro_body)
        w.mark_set("voice_anchor", "end-1c")
        w.mark_gravity("voice_anchor", "left")
        w.insert("voice_anchor", self.VOICE_CURSOR_TOKEN, ("voice_cursor_tag",))
        # Place user's typing cursor on line 2 so both cursors are visibly separate right from the start
        w.mark_set("insert", "2.0")
        self._dual_update_coord_badges(w)

    def _dual_ensure_voice_cursor_token(self, w):
        ranges = w.tag_ranges("voice_cursor_tag")
        if ranges and len(ranges) >= 2:
            return str(ranges[0]), str(ranges[1])
        try:
            pos = w.index("voice_anchor")
        except Exception:
            pos = w.index("end-1c")
            w.mark_set("voice_anchor", pos)
            w.mark_gravity("voice_anchor", "left")
        w.insert(pos, self.VOICE_CURSOR_TOKEN, ("voice_cursor_tag",))
        ranges = w.tag_ranges("voice_cursor_tag")
        return str(ranges[0]), str(ranges[1])

    def _dual_relocate_voice_cursor_in_widget(self, w, target_index: str, flash: bool = True):
        try:
            user_ins = w.index("insert")
            target_abs = w.index(target_index)
            ranges = w.tag_ranges("voice_cursor_tag")
            if ranges and len(ranges) >= 2:
                s0, e0 = str(ranges[0]), str(ranges[1])
                if w.compare(target_abs, ">=", s0) and w.compare(target_abs, "<=", e0):
                    return
                w.delete(s0, e0)
                target_abs = w.index(target_index)
            w.mark_set("voice_anchor", target_abs)
            w.mark_gravity("voice_anchor", "left")
            tags = ("voice_cursor_tag", "voice_cursor_flash") if flash else ("voice_cursor_tag",)
            w.insert("voice_anchor", self.VOICE_CURSOR_TOKEN, tags)
            w.mark_set("insert", user_ins)
            if flash:
                self.after(360, lambda w=w: w.tag_remove("voice_cursor_flash", "1.0", "end") if w.winfo_exists() else None)
            self._dual_update_coord_badges(w)
        except Exception:
            pass

    def _dual_move_voice_cursor(self, where: str):
        for w in self._dual_active_widgets():
            self._dual_relocate_voice_cursor_in_widget(w, where, flash=True)

    def _dual_insert_at_voice_cursor(self, text: str):
        if not text:
            return
        for w in self._dual_active_widgets():
            try:
                user_ins = w.index("insert")
                vc_start, _ = self._dual_ensure_voice_cursor_token(w)
                # If user's typing cursor was at or after vc_start, tk.Text automatically shifts it;
                # if user was typing right on vc_start, keep user_ins before the voice token or preserve their spot
                was_before = w.compare(user_ins, "<", vc_start)
                w.insert(vc_start, text, ("voice_recent",))
                if was_before:
                    w.mark_set("insert", user_ins)
                self.after(1200, lambda w=w: w.tag_remove("voice_recent", "1.0", "end") if w.winfo_exists() else None)
                self._dual_update_coord_badges(w)
            except Exception:
                pass

    def _dual_newline_at_voice_cursor(self):
        self._dual_insert_at_voice_cursor("\n")

    def _dual_insert_heading_at_typing_cursor(self):
        for w in self._dual_active_widgets():
            try:
                w.insert("insert", "\n【章节大标题】\n", ("heading_tag",))
                w.focus_set()
                self._dual_update_coord_badges(w)
            except Exception:
                pass

    def _dual_update_coord_badges(self, w=None):
        w = w or getattr(self, "dual_text", None)
        if w is None or not w.winfo_exists():
            return
        try:
            ins = w.index("insert")
            il, ic = ins.split(".")
            ranges = w.tag_ranges("voice_cursor_tag")
            if ranges and len(ranges) >= 2:
                vc = str(ranges[0])
            else:
                vc = w.index("end-1c")
            vl, vc_col = vc.split(".")
            if hasattr(self, "dual_type_coord_lbl"):
                self.dual_type_coord_lbl.configure(
                    text=f"⌨️ 打字光标：第 {il} 行 · 第 {int(ic) + 1} 列（左键点击任意处打字/写标题/改稿）"
                )
            if hasattr(self, "dual_voice_coord_lbl"):
                self.dual_voice_coord_lbl.configure(
                    text=f"🎙 转录光标：第 {vl} 行 · 第 {int(vc_col) + 1} 列（按住绿标拖动 / 右键点任意处 即可移位）"
                )
        except Exception:
            pass

    def _dual_clean_content(self, w=None) -> str:
        w = w or getattr(self, "dual_text", None)
        if w is None:
            return ""
        raw = w.get("1.0", "end-1c")
        return raw.replace(self.VOICE_CURSOR_TOKEN, "")

    def _dual_copy_doc(self):
        txt = self._dual_clean_content().strip()
        if txt:
            self.clipboard_clear()
            self.clipboard_append(txt)
            if hasattr(self, "dual_voice_coord_lbl"):
                self.dual_voice_coord_lbl.configure(text=f"✓ 已复制干净全文（{len(txt)} 字，已自动剥离光标标记）")
            elif hasattr(self, "mv_status_lbl"):
                self.mv_status_lbl.configure(text=f"✓ 已复制随手记全文（{len(txt)} 字）", text_color=C("success"))

    def _dual_export_doc(self):
        txt = self._dual_clean_content().strip()
        if not txt:
            return
        dst = filedialog.asksaveasfilename(
            parent=self,
            title="导出双光标协同速记文稿",
            initialfile=f"鹤羽双光标速记_{time.strftime('%Y%m%d_%H%M%S')}.md",
            defaultextension=".md",
            filetypes=[("Markdown 文档", "*.md"), ("UTF-8 文本", "*.txt"), ("所有文件", "*.*")],
        )
        if dst:
            with open(dst, "w", encoding="utf-8") as f:
                f.write(txt + "\n")
            self.dual_voice_coord_lbl.configure(text=f"✓ 已导出至：{os.path.basename(dst)}")

    def _dual_clear_canvas(self):
        for w in self._dual_active_widgets():
            w.delete("1.0", "end")
            w.mark_set("voice_anchor", "1.0")
            w.mark_gravity("voice_anchor", "left")
            w.insert("1.0", "\n")
            w.insert("end-1c", self.VOICE_CURSOR_TOKEN, ("voice_cursor_tag",))
            w.mark_set("insert", "1.0")
            self._dual_update_coord_badges(w)

    def _dual_run_live_demo(self):
        demo_chunks = [
            "【实时转录流 1】各位同事好，今天我们重点复盘二季度核心产品线的交付进度与架构升级方案。",
            "【实时转录流 2】在音频底层方面，我们已经完成了 48kHz 立体声母带级降噪以及指定进程独立内录隔离。",
            "【实时转录流 3】与此同时，您可以在上方任意位置移动打字光标添加大标题或修改纪要，两个光标分头行动、互不干扰！",
        ]
        for idx, sentence in enumerate(demo_chunks):
            self.after(idx * 950 + 100, lambda s=sentence: self._dual_insert_at_voice_cursor(s))

    def _dual_open_popout_window(self):
        if self._dual_popout_win is not None and self._dual_popout_win.winfo_exists():
            self._dual_popout_win.lift()
            self._dual_popout_win.focus_force()
            return
        win = ctk.CTkToplevel(self)
        win.title("鹤羽音形 · 双光标协同速记工作台 (⌨️打字光标 + 🎙转录光标 分头行动)")
        win.geometry("920x640")
        win.configure(fg_color=C("bg"))
        self._dual_popout_win = win

        head = ctk.CTkFrame(win, fg_color=C("surface"), corner_radius=10, border_width=1, border_color=C("border"))
        head.pack(fill="x", padx=16, pady=(14, 8))
        h_in = ctk.CTkFrame(head, fg_color="transparent")
        h_in.pack(fill="x", padx=14, pady=10)
        self._label(h_in, "✍️ 双光标协同速记大窗 · 鼠标左键任意打字改稿 / 右键点击或拖动「┃🎙转录光标」控制语音输出位置", 13, "bold").pack(side="left")
        self._btn(h_in, "📍 移回文末", lambda: self._dual_move_voice_cursor("end-1c"), height=28).pack(side="right")
        self._btn(h_in, "🎯 召唤到打字处", lambda: self._dual_move_voice_cursor("insert"), height=28).pack(side="right", padx=(0, 8))
        self._btn(h_in, "📑 插入标题", self._dual_insert_heading_at_typing_cursor, height=28).pack(side="right", padx=(0, 8))

        body = ctk.CTkFrame(win, fg_color=C("surface_sunken"), corner_radius=10, border_width=1, border_color=C("border_strong"))
        body.pack(fill="both", expand=True, padx=16, pady=(0, 16))
        ptxt = self._create_dual_cursor_text_widget(body, height=24)
        self._dual_popout_text = ptxt
        # Sync current content from main workbench
        ptxt.delete("1.0", "end")
        ptxt.insert("1.0", self._dual_clean_content(self.dual_text))
        ptxt.mark_set("voice_anchor", "end-1c")
        ptxt.mark_gravity("voice_anchor", "left")
        ptxt.insert("voice_anchor", self.VOICE_CURSOR_TOKEN, ("voice_cursor_tag",))
        ptxt.mark_set("insert", "1.0")

    def _dual_read_latest_seq(self) -> int:
        best = 0
        for d in SYNC_DATA_DIRS:
            p = os.path.join(d, "voice_stream.jsonl")
            if os.path.exists(p):
                try:
                    with open(p, "r", encoding="utf-8") as f:
                        for line in f:
                            line = line.strip()
                            if line:
                                obj = json.loads(line)
                                best = max(best, int(obj.get("seq", 0)))
                except Exception:
                    pass
        return best

    def _dual_on_sink_toggle(self):
        self._dual_write_sink_heartbeat()

    def _dual_write_sink_heartbeat(self):
        popout_open = getattr(self, "_dual_popout_win", None) is not None and self._dual_popout_win.winfo_exists()
        is_mini_voice = getattr(self, "mini_mode", None) == "voice"
        active = bool(hasattr(self, "dual_sink_sw") and self.dual_sink_sw.get() and (is_mini_voice or self.current_page == "voice" or popout_open))
        for d in SYNC_DATA_DIRS:
            try:
                _write_json(os.path.join(d, "dual_cursor_sink.json"), {"active": active, "ts": time.time()})
            except Exception:
                pass

    def _dual_stream_poll_tick(self):
        try:
            self._dual_write_sink_heartbeat()
            new_items = []
            for d in SYNC_DATA_DIRS:
                p = os.path.join(d, "voice_stream.jsonl")
                if not os.path.exists(p):
                    continue
                with open(p, "r", encoding="utf-8") as f:
                    for line in f:
                        line = line.strip()
                        if not line:
                            continue
                        try:
                            obj = json.loads(line)
                            seq = int(obj.get("seq", 0))
                            if seq > self._dual_last_seq:
                                new_items.append((seq, str(obj.get("text", ""))))
                        except Exception:
                            pass
                break
            if new_items:
                new_items.sort(key=lambda x: x[0])
                for seq, text in new_items:
                    if seq > self._dual_last_seq:
                        self._dual_last_seq = seq
                        self._dual_insert_at_voice_cursor(text)
        except Exception:
            pass
        self.after(150, self._dual_stream_poll_tick)

    # ---- Studio Recorder & Built-in Audio Preview Implementation -------------
    def _studio_preview_stop(self):
        try:
            winsound.PlaySound(None, 0)
        except Exception:
            pass
        self._studio_preview_playing = False
        if hasattr(self, "rec_play_btn"):
            self.rec_play_btn.configure(text="试听", image=self._ui_icon("play", (14, 14)))

    def _studio_preview_toggle(self):
        if self._studio_rec_proc is not None and self._studio_rec_proc.poll() is None:
            messagebox.showinfo("正在录制", "请先点击「停止录制」结束当前录制，再试听录音效果。", parent=self)
            return
        if self._studio_preview_playing:
            self._studio_preview_stop()
            if os.path.exists(self._studio_staging_wav):
                self.rec_timer_lbl.configure(text="⏹ 已停止试听 · 不满意可点「重新录制」或「放弃」", text_color=C("text_2"))
            return
        if not os.path.exists(self._studio_staging_wav) or os.path.getsize(self._studio_staging_wav) <= 44:
            messagebox.showinfo("试听录音", "当前没有可试听的录音。请先点击「开始录制」录制一段声音。", parent=self)
            return
        flags = winsound.SND_FILENAME | winsound.SND_ASYNC
        if self.rec_loop_sw.get():
            flags |= winsound.SND_LOOP
        try:
            winsound.PlaySound(self._studio_staging_wav, flags)
            self._studio_preview_playing = True
            self._studio_preview_start_ts = time.time()
            self.rec_play_btn.configure(text="停止", image=self._ui_icon("stop_wave", (14, 14)))
            self._studio_preview_tick()
        except Exception as e:
            messagebox.showwarning("试听失败", f"无法播放试听音频：{e}", parent=self)

    def _studio_preview_on_loop_toggle(self):
        if self._studio_preview_playing and os.path.exists(self._studio_staging_wav):
            flags = winsound.SND_FILENAME | winsound.SND_ASYNC
            if self.rec_loop_sw.get():
                flags |= winsound.SND_LOOP
            try:
                winsound.PlaySound(self._studio_staging_wav, flags)
                self._studio_preview_start_ts = time.time()
            except Exception:
                pass

    def _studio_preview_tick(self):
        if not self._studio_preview_playing:
            return
        elapsed = time.time() - self._studio_preview_start_ts
        dur = max(0.5, self._studio_last_duration_s)
        if not self.rec_loop_sw.get() and elapsed >= dur + 0.35:
            self._studio_preview_stop()
            self.rec_timer_lbl.configure(text=f"✓ 试听完毕（全长 {dur:.1f}s）· 不满意可点「重新录制」或「放弃」", text_color=C("success"))
            return
        mode_s = "🔁 正在循环试听" if self.rec_loop_sw.get() else "🎧 正在试听录音"
        pos_s = elapsed % dur if self.rec_loop_sw.get() else min(elapsed, dur)
        self.rec_timer_lbl.configure(text=f"{mode_s}  {pos_s:04.1f}s / {dur:04.1f}s", text_color=C("accent"))
        self.after(120, self._studio_preview_tick)

    def _studio_rec_toggle(self):
        if self._studio_rec_proc is not None and self._studio_rec_proc.poll() is None:
            self._studio_rec_stop_internal(discard=False)
            self.rec_start_btn.configure(text="重新录制", image=self._ui_icon("refresh", (15, 15)))
            if os.path.exists(self._studio_staging_wav) and os.path.getsize(self._studio_staging_wav) > 44:
                mb = os.path.getsize(self._studio_staging_wav) / 1048576.0
                rec_dir = self._get_recordings_dir()
                src_tag = "麦克风" if self._studio_src_var.get() == "mic" else ("电脑声" if self._studio_src_var.get() == "system" else "麦+电脑")
                fname = f"鹤羽录音_{src_tag}_{time.strftime('%Y%m%d_%H%M%S')}.wav"
                dst = os.path.join(rec_dir, fname)
                try:
                    shutil.copy2(self._studio_staging_wav, dst)
                    self._last_saved_wav = dst
                except Exception:
                    self._last_saved_wav = self._studio_staging_wav
                self.rec_timer_lbl.configure(
                    text=f"✓ 已录好 {self._studio_last_duration_s:.1f}s ({mb:.2f} MB) · 不满意可点「重新录制」或「放弃」",
                    text_color=C("success"),
                )
                self.rec_preview_lbl.configure(
                    text=f"📂 已保存在：{self._last_saved_wav} （点击直接打开定位）",
                    text_color=C("accent"),
                )
            return
        # If re-recording over a previous take that the user wants to replace, remove the previous take first
        if getattr(self, "_last_saved_wav", None) and os.path.isfile(self._last_saved_wav):
            try:
                _vault_secure_wipe(self._last_saved_wav)
            except Exception:
                pass
            self._last_saved_wav = None
        self._studio_preview_stop()
        if not os.path.exists(VOICE_EXE_PATH):
            messagebox.showwarning("专业录音机", f"未找到语音与录音组件：{VOICE_EXE_PATH}", parent=self)
            return
        for p in (self._studio_stop_file, self._studio_status_file):
            try:
                if os.path.exists(p):
                    os.remove(p)
            except Exception:
                pass
        cmd = [
            VOICE_EXE_PATH,
            "--record",
            "--out", self._studio_staging_wav,
            "--source", self._studio_src_var.get(),
            "--target-pid", str(getattr(self, "_studio_target_pid", 0)),
            "--stop-file", self._studio_stop_file,
            "--status-file", self._studio_status_file,
        ]
        if self.rec_denoise_sw.get():
            cmd.append("--denoise")
        if self.rec_focus_sw.get():
            cmd.append("--voice-focus")
        try:
            self._studio_rec_proc = subprocess.Popen(cmd, creationflags=0x08000000)
            self._studio_last_duration_s = 0.0
            self.rec_start_btn.configure(text="停止录制", image=self._ui_icon("stop_wave", (15, 15)))
            self.rec_timer_lbl.configure(text="🔴 正在启动 48kHz 母带录音棚…", text_color=C("danger"))
            self.rec_preview_lbl.configure(text=f"📂 录音保存位置：{self._get_recordings_dir()} （停止后自动生成 WAV）", text_color=C("muted"))
            self.after(80, self._studio_rec_poll)
        except Exception as e:
            messagebox.showwarning("专业录音机", f"启动录制失败：{e}", parent=self)

    def _studio_rec_poll(self):
        if self._studio_rec_proc is None:
            return
        if self._studio_rec_proc.poll() is not None:
            self._studio_rec_proc = None
            self.rec_start_btn.configure(text="开始录制", image=self._ui_icon("rec_badge", (15, 15)))
            self.rec_meter_bar.set(0.0)
            return
        st = _read_json(self._studio_status_file, None)
        if isinstance(st, dict):
            sec = float(st.get("seconds", 0.0))
            self._studio_last_duration_s = sec
            peak_db = float(st.get("peak_db", -90.0))
            mm, ss = int(sec) // 60, int(sec) % 60
            dsp_tag = "降噪+人声母带" if (self.rec_denoise_sw.get() and self.rec_focus_sw.get()) else ("智能降噪" if self.rec_denoise_sw.get() else ("人声母带" if self.rec_focus_sw.get() else "原声直通"))
            pid_tag = f" · 独立应用PID:{self._studio_target_pid}" if getattr(self, "_studio_target_pid", 0) > 0 else ""
            self.rec_timer_lbl.configure(text=f"🔴 正在录制  {mm:02d}:{ss:02d}.{int((sec % 1) * 10)}  ·  48kHz 立体声 ({dsp_tag}{pid_tag})", text_color=C("danger"))
            self.rec_db_lbl.configure(text=f"峰值 {peak_db:+.1f} dB")
            norm = max(0.0, min(1.0, (peak_db + 55.0) / 55.0))
            self.rec_meter_bar.set(norm)
        self.after(80, self._studio_rec_poll)

    def _studio_rec_stop_internal(self, discard: bool):
        self._studio_preview_stop()
        if self._studio_rec_proc is not None and self._studio_rec_proc.poll() is None:
            try:
                with open(self._studio_stop_file, "w", encoding="utf-8") as f:
                    f.write("discard" if discard else "stop")
                self._studio_rec_proc.wait(timeout=3.0)
            except Exception:
                try:
                    self._studio_rec_proc.terminate()
                except Exception:
                    pass
        self._studio_rec_proc = None
        self.rec_start_btn.configure(text="开始录制", image=self._ui_icon("rec_badge", (15, 15)))
        self.rec_meter_bar.set(0.0)
        if discard:
            _vault_secure_wipe(self._studio_staging_wav)

    def _studio_rec_stop_save(self):
        self._studio_rec_stop_internal(discard=False)
        if not os.path.exists(self._studio_staging_wav) or os.path.getsize(self._studio_staging_wav) <= 44:
            messagebox.showinfo("保存录音", "当前没有已录制的音频文件。", parent=self)
            return
        default_name = f"鹤羽专业录音_{time.strftime('%Y%m%d_%H%M%S')}.wav"
        dst = filedialog.asksaveasfilename(
            parent=self,
            title="保存 48kHz 立体声高保真录音",
            initialfile=default_name,
            defaultextension=".wav",
            filetypes=[("WAV 无损音频 (48kHz Stereo)", "*.wav"), ("所有文件", "*.*")],
        )
        if not dst:
            self.rec_timer_lbl.configure(text="⏸ 未保存 · 录音仍暂存在缓冲区，可继续试听或保存", text_color=C("accent"))
            return
        try:
            shutil.copy2(self._studio_staging_wav, dst)
            _vault_secure_wipe(self._studio_staging_wav)
            self.rec_timer_lbl.configure(text=f"✓ 已保存至：{os.path.basename(dst)}（缓存已安全销毁）", text_color=C("success"))
            self.rec_preview_lbl.configure(text="录音文件已导出保存，临时缓存音频已全零覆写清除。", text_color=C("muted"))
        except Exception as e:
            messagebox.showwarning("保存失败", f"无法保存文件：{e}", parent=self)

    def _studio_rec_stop_transcribe_and_wipe(self):
        self._studio_rec_stop_internal(discard=False)
        if not os.path.exists(self._studio_staging_wav) or os.path.getsize(self._studio_staging_wav) <= 44:
            messagebox.showinfo("转文字后销毁", "当前没有已录制的音频可供转写。", parent=self)
            return
        self.rec_timer_lbl.configure(text="⏳ 正在离线转写录音内容，完成后将自动全零覆写销毁原音频…", text_color=C("accent"))
        self._run_batch_transcribe(self._studio_staging_wav, wipe_after=True)

    def _studio_rec_discard(self):
        self._studio_rec_stop_internal(discard=True)
        _vault_secure_wipe(self._studio_staging_wav)
        if getattr(self, "_last_saved_wav", None) and os.path.isfile(self._last_saved_wav):
            _vault_secure_wipe(self._last_saved_wav)
        self._last_saved_wav = None
        if hasattr(self, "rec_start_btn"):
            self.rec_start_btn.configure(text="开始录制", image=self._ui_icon("rec_badge", (15, 15)))
        self.rec_timer_lbl.configure(text="🗑 已放弃并彻底删除刚才的录音 · 可随时重新录制", text_color=C("danger"))
        self.rec_db_lbl.configure(text="峰值 -∞ dB")
        self.rec_preview_lbl.configure(
            text=f"📂 录音保存位置：{self._get_recordings_dir()} （点击此处或「文件夹」直接打开）",
            text_color=C("text_2"),
        )

    # ---- Voice Replace Rules & Conflict Checker (ported from TermsLibraryManager) ----
    @staticmethod
    def _parse_and_check_replace_rules(raw_rules: str):
        pairs = []
        warnings = []
        wrong_to_correct = {}
        all_wrongs = set()
        for seg in raw_rules.replace("；", ";").replace("\r", "\n").replace("\n", ";").split(";"):
            seg = seg.strip()
            if not seg or seg.startswith("#"):
                continue
            arrow = next((a for a in ("=>", "->", "＝>", "→") if a in seg), None)
            if not arrow:
                warnings.append(f"格式提示：「{seg}」缺少箭头 '=>'")
                continue
            lhs, rhs = seg.split(arrow, 1)
            correct = rhs.strip()
            if not correct:
                warnings.append(f"格式提示：「{seg}」缺少右侧正词")
                continue
            for w_raw in lhs.replace("，", ",").replace("|", ",").split(","):
                wrong = w_raw.strip()
                if not wrong:
                    continue
                if wrong == correct:
                    warnings.append(f"「{wrong} => {correct}」无效：错词与正词相同")
                    continue
                if wrong in wrong_to_correct:
                    prev = wrong_to_correct[wrong]
                    if prev != correct:
                        warnings.append(f"多义冲突：「{wrong}」同时指向「{prev}」和「{correct}」")
                    else:
                        warnings.append(f"重复定义：「{wrong} => {correct}」")
                else:
                    wrong_to_correct[wrong] = correct
                    pairs.append((wrong, correct))
                all_wrongs.add(wrong)
        for wrong, correct in wrong_to_correct.items():
            if correct in all_wrongs:
                chained = wrong_to_correct.get(correct, "")
                if chained == wrong:
                    if wrong < correct:
                        warnings.append(f"循环替换冲突：「{wrong} => {correct}」与「{correct} => {wrong}」互为循环")
                elif chained:
                    warnings.append(f"链式依赖提示：「{wrong} => {correct} => {chained}」（已由占位符保护防二次替换）")
        return pairs, warnings

    def _update_voice_rules_status_label(self, raw_rules: str):
        if not hasattr(self, "voice_rules_status_lbl"):
            return
        pairs, warnings = self._parse_and_check_replace_rules(raw_rules)
        if not raw_rules.strip():
            self.voice_rules_status_lbl.configure(
                text="当前未配置自定义错词替换规则（也可在「词库」页中添加 错词=>正词 词条，享受 256-bit 密文保护）",
                text_color=C("muted"),
            )
        elif warnings:
            self.voice_rules_status_lbl.configure(
                text=f"⚠ 已解析 {len(pairs)} 条规则 · 发现 {len(warnings)} 项提示：{warnings[0]}",
                text_color=C("danger"),
            )
        else:
            self.voice_rules_status_lbl.configure(
                text=f"✓ 已生效 {len(pairs)} 条替换规则 · 最长匹配优先 · 零冲突",
                text_color=C("success"),
            )

    def _save_voice_replace_rules(self):
        if not hasattr(self, "voice_replace_rules_entry"):
            return
        raw = self.voice_replace_rules_entry.get().strip()
        self._set_voice("replace_rules", raw)
        self._update_voice_rules_status_label(raw)

    def _check_voice_replace_rules_ui(self):
        if not hasattr(self, "voice_replace_rules_entry"):
            return
        raw = self.voice_replace_rules_entry.get().strip()
        pairs, warnings = self._parse_and_check_replace_rules(raw)
        self._update_voice_rules_status_label(raw)
        if not raw:
            messagebox.showinfo("替换规则冲突体检", "当前尚未填写自定义替换规则。\n\n示例格式：\n鹤雨,河宇=>鹤羽; 云计算平台=>云平台", parent=self)
        elif warnings:
            messagebox.showwarning(
                "替换规则冲突体检",
                f"共解析出 {len(pairs)} 条有效映射，检测到以下 {len(warnings)} 项冲突或提示：\n\n• " + "\n• ".join(warnings),
                parent=self,
            )
        else:
            messagebox.showinfo(
                "替换规则冲突体检",
                f"✓ 体检通过！共 {len(pairs)} 条替换规则，无任何同词冲突、多义映射或循环依赖。",
                parent=self,
            )

    # ---- Batch Audio File Transcription Implementation -----------------------
    def _batch_transcribe_pick_file(self):
        paths = filedialog.askopenfilenames(
            parent=self,
            title="选择要批量转成文字的音频文件（支持多选）",
            filetypes=[("音频文件", "*.wav;*.mp3;*.m4a;*.flac;*.ogg;*.aac;*.mp4"), ("所有文件", "*.*")],
        )
        if not paths:
            return
        self._run_batch_transcribe(list(paths), wipe_after=False)

    def _run_batch_transcribe(self, wav_paths, wipe_after: bool = False):
        if not os.path.exists(VOICE_EXE_PATH):
            messagebox.showwarning("离线转写", f"未找到语音引擎：{VOICE_EXE_PATH}", parent=self)
            return
        if isinstance(wav_paths, str):
            wav_paths = [wav_paths]
        if not wav_paths:
            return
        total = len(wav_paths)
        out_txt = os.path.join(DATA_DIR, "tmp_batch_transcript.txt")
        try:
            if os.path.exists(out_txt):
                os.remove(out_txt)
        except Exception:
            pass
        if hasattr(self, "batch_status_lbl"):
            self.batch_status_lbl.configure(
                text=f"⏳ [1/{total}] 正在转写：{os.path.basename(wav_paths[0])} …" if total > 1 else f"⏳ 正在转写：{os.path.basename(wav_paths[0])} …",
                text_color=C("accent"),
            )
        if hasattr(self, "batch_text_box"):
            self.batch_text_box.configure(state="normal")
            self.batch_text_box.delete("1.0", "end")
            self.batch_text_box.insert("end", "正在调用本地 Whisper 引擎转写音频，请稍候…")
            self.batch_text_box.configure(state="disabled")

        def work():
            t0 = time.time()
            results = []
            last_rc = 0
            last_stdout = ""
            for idx, wp in enumerate(wav_paths):
                if total > 1 and hasattr(self, "batch_status_lbl"):
                    self.after(
                        0,
                        lambda i=idx + 1, name=os.path.basename(wp): self.batch_status_lbl.configure(
                            text=f"⏳ [{i}/{total}] 正在批量转写：{name} …",
                            text_color=C("accent"),
                        ),
                    )
                try:
                    if os.path.exists(out_txt):
                        os.remove(out_txt)
                except Exception:
                    pass
                proc = subprocess.run(
                    [VOICE_EXE_PATH, "--transcribe-file", wp, "--out-file", out_txt],
                    capture_output=True, text=True, encoding="utf-8", errors="replace", creationflags=0x08000000,
                )
                last_rc = proc.returncode
                last_stdout = proc.stdout
                txt = ""
                if os.path.exists(out_txt):
                    try:
                        txt = open(out_txt, "r", encoding="utf-8").read().strip()
                    except Exception:
                        pass
                    _vault_secure_wipe(out_txt)
                if wipe_after:
                    _vault_secure_wipe(wp)
                if txt:
                    results.append(f"【{os.path.basename(wp)}】\n{txt}" if total > 1 else txt)
            joined = "\n\n".join(results).strip()
            return last_rc, joined, last_stdout, time.time() - t0

        def done(res):
            if not res:
                if hasattr(self, "batch_status_lbl"):
                    self.batch_status_lbl.configure(text="转写执行异常", text_color=C("danger"))
                return
            rc, txt, stdout, elapsed = res
            if hasattr(self, "batch_text_box"):
                self.batch_text_box.configure(state="normal")
                self.batch_text_box.delete("1.0", "end")
            if txt:
                if hasattr(self, "batch_text_box"):
                    self.batch_text_box.insert("end", txt)
                self.clipboard_clear()
                self.clipboard_append(txt)
                if wipe_after:
                    self.rec_timer_lbl.configure(text="✓ 已转成文字并复制，原录音文件已全零覆写彻底销毁！", text_color=C("success"))
                    if hasattr(self, "batch_status_lbl"):
                        self.batch_status_lbl.configure(text=f"✓ 转写完成（耗时 {elapsed:.1f}s · 已自动复制到剪贴板 · 原音频已永久销毁）", text_color=C("success"))
                elif hasattr(self, "batch_status_lbl"):
                    self.batch_status_lbl.configure(text=f"✓ 转写完成（耗时 {elapsed:.1f}s · 共 {len(txt)} 字 · 已自动复制到剪贴板）", text_color=C("success"))
            else:
                if hasattr(self, "batch_text_box"):
                    self.batch_text_box.insert("end", txt or f"未能识别出有效语音内容。\n详细输出：{(stdout or '').strip()}")
                if hasattr(self, "batch_status_lbl"):
                    self.batch_status_lbl.configure(text="未检测到有效人声或模型未下载", text_color=C("danger"))
            if hasattr(self, "batch_text_box"):
                self.batch_text_box.configure(state="disabled")

        self._run_bg(work, done)

    def _batch_copy_text(self):
        txt = self.batch_text_box.get("1.0", "end").strip()
        if txt:
            self.clipboard_clear()
            self.clipboard_append(txt)
            self.batch_status_lbl.configure(text=f"✓ 已复制全文（{len(txt)} 字）到剪贴板", text_color=C("success"))

    def _batch_save_txt(self):
        txt = self.batch_text_box.get("1.0", "end").strip()
        if not txt:
            return
        dst = filedialog.asksaveasfilename(
            parent=self,
            title="保存转写文本",
            initialfile=f"鹤羽语音转写_{time.strftime('%Y%m%d_%H%M%S')}.txt",
            defaultextension=".txt",
            filetypes=[("UTF-8 文本文件", "*.txt"), ("所有文件", "*.*")],
        )
        if dst:
            with open(dst, "w", encoding="utf-8") as f:
                f.write(txt + "\n")
            self.batch_status_lbl.configure(text=f"✓ 已保存至：{os.path.basename(dst)}", text_color=C("success"))

    def _on_show_voice(self):
        self._refresh_model_status()

    def _model_path(self, mid):
        return os.path.join(MODELS_DIR, f"ggml-{mid}.bin")

    def _refresh_model_status(self):
        for mid, (status, dl, radio) in getattr(self, "_model_rows", {}).items():
            ok = os.path.exists(self._model_path(mid))
            if ok:
                try:
                    mb = os.path.getsize(self._model_path(mid)) / 1048576
                    status.configure(text=f"已安装 · {mb:.0f} MB", text_color=C("success"))
                except Exception:
                    status.configure(text="已安装", text_color=C("success"))
                if dl.winfo_ismapped():
                    dl.pack_forget()
            else:
                status.configure(text="未下载", text_color=C("muted"))
                if not dl.winfo_ismapped():
                    dl.pack(side="left", padx=(0, 16), before=radio)

    def _find_fetch_script(self):
        for base in (BASE_DIR, INSTALL_DIR):
            p = os.path.join(base, "tools", "fetch_voice_model.ps1")
            if os.path.exists(p):
                return p
        return None

    def _download_model(self, mid):
        script = self._find_fetch_script()
        if not script:
            messagebox.showinfo(
                "下载模型",
                "模型下载脚本 tools\\fetch_voice_model.ps1 尚未安装。\n\n"
                f"您也可以手动下载 ggml-{mid}.bin（Hugging Face：ggerganov/whisper.cpp），\n"
                f"并放到：{MODELS_DIR}",
                parent=self,
            )
            return
        try:
            subprocess.Popen(
                ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script, "-Model", mid],
                creationflags=subprocess.CREATE_NEW_CONSOLE,
            )
        except Exception as e:
            messagebox.showwarning("下载模型", f"无法启动下载脚本：{e}", parent=self)

    # =========================================================================
    # PAGE ④ 外观 APPEARANCE (含统一字体大小控制 + 候选框实时效果预览条)
    # =========================================================================
    def _build_page_appearance(self):
        page, col = self._page("外观", "统一控制所有皮肤的候选框字体大小、实时预览无锯齿圆角效果、切换主题与排列方向")
        self.theme_meta = self._load_theme_meta()

        # ---- 1. Live Candidate Bar Preview (实时效果预览条)
        g_prev = self._group(
            col,
            "✨ 候选框实时效果预览条（字号与皮肤实时联动 · C1 超采样无锯齿圆角）",
            note="调整下方统一字号或点击任意皮肤，此处实时还原真实候选框的字号、配色与柔和圆角",
            pady_top=14,
        )
        prev_host = ctk.CTkFrame(g_prev, fg_color="transparent")
        prev_host.pack(fill="x", padx=ROW_PX, pady=(14, 14))
        self.hud_live_card = ctk.CTkFrame(
            prev_host,
            fg_color="#FFFFFF",
            corner_radius=14,
            border_width=1,
            border_color="#D0D7DE",
        )
        self.hud_live_card.pack(anchor="w", ipadx=14, ipady=8)
        self.hud_live_code_lbl = ctk.CTkLabel(
            self.hud_live_card,
            text="heyu  ▎ 鹤羽音形 · 四码定长",
            font=self.F(13, "bold", mono=True),
            text_color="#2563EB",
            anchor="w",
        )
        self.hud_live_code_lbl.pack(anchor="w", padx=16, pady=(6, 2))
        self.hud_live_cands_row = ctk.CTkFrame(self.hud_live_card, fg_color="transparent")
        self.hud_live_cands_row.pack(anchor="w", padx=14, pady=(2, 6))
        self.hud_live_first_pill = ctk.CTkLabel(
            self.hud_live_cands_row,
            text=" 1. 鹤羽音形 ",
            corner_radius=8,
            fg_color="#DBEAFE",
            text_color="#1D4ED8",
            font=self.F(18, "bold"),
        )
        self.hud_live_first_pill.pack(side="left", padx=(0, 10), ipady=2)
        self.hud_live_rest_lbl = ctk.CTkLabel(
            self.hud_live_cands_row,
            text="2. 鹤羽   3. 何与   4. 合于   [IT] piuma di gru",
            font=self.F(17),
            text_color="#1F2328",
            anchor="w",
        )
        self.hud_live_rest_lbl.pack(side="left")

        # ---- 2. Unified Font Size Control Across All Themes (统一控制预览条字体大小)
        g = self._group(col, "候选框统一字体大小与排版（跨全部 9 款皮肤统一生效）")
        fs = max(0.75, min(2.20, float(self.cfg.get("font_scale", 1.0))))
        r_fs = self._row(
            g,
            "统一字体大小缩放",
            "一键统一控制所有皮肤主题的候选词、编码行与外语伴学字号（快捷键 Ctrl+Shift+= 放大 / Ctrl+Shift+- 缩小）",
            stacked=True,
            badge="全局统一控制",
        )
        fs_ctrl = ctk.CTkFrame(r_fs.slot, fg_color="transparent")
        fs_ctrl.pack(fill="x")
        for label_txt, scale_val in [
            ("紧凑 85%", 0.85),
            ("标准 100%", 1.00),
            ("舒适 115%", 1.15),
            ("清晰 130%", 1.30),
            ("大字 150%", 1.50),
            ("特大 175%", 1.75),
        ]:
            self._btn(
                fs_ctrl,
                label_txt,
                lambda v=scale_val: self._apply_unified_font_scale(v, source="preset"),
                height=28,
            ).pack(side="left", padx=(0, 6))

        fs_slider_row = ctk.CTkFrame(r_fs.slot, fg_color="transparent")
        fs_slider_row.pack(fill="x", pady=(8, 0))
        self._btn(
            fs_slider_row,
            "➖ 缩小",
            lambda: self._apply_unified_font_scale(float(self.cfg.get("font_scale", 1.0)) - 0.05, source="step"),
            height=28,
            width=68,
        ).pack(side="left", padx=(0, 8))
        self.app_font_slider = ctk.CTkSlider(
            fs_slider_row,
            from_=0.75,
            to=2.20,
            number_of_steps=29,
            width=260,
            height=18,
            fg_color=C("seg_bg"),
            progress_color=C("accent"),
            button_color=C("accent"),
            button_hover_color=C("accent_hover"),
            command=lambda v: self._apply_unified_font_scale(float(v), source="appearance"),
        )
        self.app_font_slider.set(fs)
        self.app_font_slider.pack(side="left")
        self._btn(
            fs_slider_row,
            "➕ 放大",
            lambda: self._apply_unified_font_scale(float(self.cfg.get("font_scale", 1.0)) + 0.05, source="step"),
            height=28,
            width=68,
        ).pack(side="left", padx=(8, 12))
        self.lbl_app_font_scale = self._label(
            fs_slider_row, f"{int(round(fs * 100))}%", 15, "bold", color="accent", mono=True, width=64
        )
        self.lbl_app_font_scale.pack(side="left")

        r = self._row(g, "排列方向", "候选词横向或纵向排列")
        self.layout_seg = self._choice(r.slot, [("横排", "horizontal"), ("竖排", "vertical")],
                                       self.hud_style.get("layout", "horizontal"), self._on_layout_change)
        self.layout_seg.pack()
        r = self._row(g, "毛玻璃效果", "启用柔和亚克力背景（已修复 DWM 半透明直角框与圆角锯齿问题）", badge="无锯齿圆角")
        self.glass_sw = self._switch(r.slot, bool(self.hud_style.get("glass")), self._on_glass_toggle)
        self.glass_sw.pack()

        g = self._group(col, "设置中心")
        r = self._row(g, "界面外观", "仅影响本设置窗口，不影响打字候选框主题")
        cur = _ui_prefs.get("appearance", "system")
        self.appearance_seg = self._choice(r.slot, [(l, k) for l, k in _APPEARANCE_LABELS], cur, self._on_appearance_change)
        self.appearance_seg.pack()

        # theme gallery with category filter (Illustrated Art Series + Classic Series)
        hdr = ctk.CTkFrame(col, fg_color="transparent")
        hdr.pack(fill="x", pady=(24, 8))
        self._label(hdr, "候选框与状态胶囊主题画廊（含 6 款全新艺术插画花草动物主题 + 9 款经典主题）", 13, "bold", color="text").pack(side="left", padx=(4, 0))
        self._label(hdr, f"共 {len(self.theme_meta)} 款 · 胶囊可放「彩」一键轮换", 12, color="muted").pack(side="right", padx=(0, 4))

        cat_bar = ctk.CTkFrame(col, fg_color=C("surface"), corner_radius=10, border_width=1, border_color=C("border"))
        cat_bar.pack(fill="x", pady=(0, 10))
        cb_in = ctk.CTkFrame(cat_bar, fg_color="transparent")
        cb_in.pack(fill="x", padx=12, pady=8)
        self._label(cb_in, "主题风格筛选：", 12, "bold", color="muted").pack(side="left", padx=(2, 8))
        self._theme_cat_filter = "all"
        self._theme_cat_btns = {}
        for c_key, c_lbl in [
            ("all", "🌟 全部主题 (15款)"),
            ("art", "🎨 艺术插画 · 花草动物系列 (6款全新)"),
            ("classic", "✨ 经典极简 · 毛玻璃与纸感系列 (9款保留)"),
        ]:
            b = ctk.CTkButton(
                cb_in,
                text=c_lbl,
                height=28,
                width=0,
                corner_radius=7,
                font=self.F(12, "bold"),
                fg_color=C("accent") if c_key == "all" else C("surface_alt"),
                hover_color=C("accent_hover") if c_key == "all" else C("nav_hover"),
                text_color=C("on_accent") if c_key == "all" else C("text"),
                command=lambda k=c_key: self._set_theme_cat_filter(k),
            )
            b.pack(side="left", padx=3)
            self._theme_cat_btns[c_key] = b

        self.gallery = ctk.CTkFrame(col, fg_color="transparent")
        self.gallery.pack(fill="x")
        for c in range(3):
            self.gallery.grid_columnconfigure(c, weight=1, uniform="gal")
        self._populate_gallery()
        self._update_live_hud_preview(fs)
        return page

    def _set_theme_cat_filter(self, cat_key: str):
        self._theme_cat_filter = cat_key
        for k, b in getattr(self, "_theme_cat_btns", {}).items():
            sel = (k == cat_key)
            b.configure(
                fg_color=C("accent") if sel else C("surface_alt"),
                hover_color=C("accent_hover") if sel else C("nav_hover"),
                text_color=C("on_accent") if sel else C("text"),
            )
        self._populate_gallery()

    def _update_live_hud_preview(self, scale_val=None):
        if not hasattr(self, "hud_live_card"):
            return
        if scale_val is None:
            scale_val = float(self.cfg.get("font_scale", 1.0))
        scale_val = max(0.75, min(2.20, float(scale_val)))
        t_idx = int(self.cfg.get("theme_index", 0))
        meta = getattr(self, "theme_meta", None) or self._load_theme_meta()
        t_info = next((m for m in meta if m.get("index") == t_idx), meta[0] if meta else {})
        bg = t_info.get("bg") or "#FFFFFF"
        acc = t_info.get("accent") or "#2563EB"
        is_dark = bool(t_info.get("is_dark", False))
        txt_col = "#F0F6FC" if is_dark else "#1F2328"
        pill_bg = "#1E3A5F" if is_dark else "#DBEAFE"
        border_col = "#3D444D" if is_dark else "#D0D7DE"
        code_sz = max(10, int(round(13 * scale_val)))
        cand_sz = max(13, int(round(18 * scale_val)))
        self.hud_live_card.configure(fg_color=bg, border_color=border_col)
        self.hud_live_code_lbl.configure(
            font=self.F(code_sz, "bold", mono=True),
            text_color=acc,
            text=f"heyu  ▎ {t_info.get('name', '主题').split(' · ')[0]} · 字号 {int(round(scale_val * 100))}%",
        )
        self.hud_live_first_pill.configure(
            font=self.F(cand_sz, "bold"),
            fg_color=pill_bg,
            text_color=acc if not is_dark else "#93C5FD",
        )
        self.hud_live_rest_lbl.configure(
            font=self.F(max(12, cand_sz - 1)),
            text_color=txt_col,
        )

    def _on_layout_change(self, key):
        self.hud_style["layout"] = key
        self._save_hud_style()

    def _on_glass_toggle(self):
        self.hud_style["glass"] = bool(self.glass_sw.get())
        self._save_hud_style()

    def _on_appearance_change(self, key):
        ctk.set_appearance_mode(_APPEARANCE_MAP[key])
        _ui_prefs["appearance"] = key
        _write_json(STUDIO_UI_PATH, _ui_prefs)
        self.after(10, self._apply_tree_style)

    THUMB_W, THUMB_H = 236, 150

    def _populate_gallery(self):
        for w in self.gallery.winfo_children():
            w.destroy()
        self._gallery_cards = {}
        self._gallery_thumbs = {}
        cat = getattr(self, "_theme_cat_filter", "all")
        # Order: show the 6 new illustrated themes first when in "all" or "art" so user sees them right away,
        # while keeping all 9 classic themes right below!
        art_list = [t for t in self.theme_meta if t.get("index", 0) >= 9 or t.get("art")]
        classic_list = [t for t in self.theme_meta if not (t.get("index", 0) >= 9 or t.get("art"))]
        if cat == "art":
            display_list = art_list
        elif cat == "classic":
            display_list = classic_list
        else:
            display_list = art_list + classic_list

        for pos, t in enumerate(display_list):
            idx = t["index"]
            is_art = bool(t.get("index", 0) >= 9 or t.get("art"))
            card = self._card(self.gallery)
            card.grid(row=pos // 3, column=pos % 3, sticky="nsew",
                      padx=(0 if pos % 3 == 0 else 8, 0 if pos % 3 == 2 else 8), pady=8)
            thumb = ctk.CTkFrame(card, fg_color=t.get("bg") or "#222222", corner_radius=8, height=self.THUMB_H)
            thumb.pack(fill="x", padx=10, pady=(10, 0))
            thumb.pack_propagate(False)
            img_lbl = ctk.CTkLabel(thumb, text="", fg_color="transparent")
            img_lbl.place(relx=0.5, rely=0.5, anchor="center")
            info = ctk.CTkFrame(card, fg_color="transparent")
            info.pack(fill="x", padx=12, pady=(10, 12))
            ctk.CTkFrame(info, width=10, height=10, corner_radius=5, fg_color=t.get("accent") or "#888888").pack(side="left", padx=(0, 7))
            name = t.get("name", f"Theme {idx}")
            short = name.split(" · ")[0]
            self._label(info, short, 13, "bold").pack(side="left")
            sub_tag = t.get("tag") or ("艺术插画" if is_art else ("深色" if t.get("is_dark") else "浅色"))
            self._label(info, sub_tag, 11, color="accent" if is_art else "muted").pack(side="left", padx=(6, 0))
            check = self._label(info, "✓ 使用中", 12, "bold", color="accent")
            self._gallery_cards[idx] = (card, check)
            self._gallery_thumbs[idx] = img_lbl
            for w in (card, thumb, img_lbl, info) + tuple(info.winfo_children()):
                targets = [w, getattr(w, "_canvas", None), getattr(w, "_label", None)]
                for tgt in targets:
                    if tgt is not None:
                        try:
                            tgt.bind("<Button-1>", lambda e, i=idx: self._set_theme(i), add="+")
                            tgt.configure(cursor="hand2")
                        except Exception:
                            pass
        self._refresh_theme_cards()
        self._load_thumbs_async()

    @staticmethod
    def _render_illustrated_theme_preview(t_meta: dict, tw: int, th: int):
        """Render a crisp 2x supersampled candidate card + botanical/animal illustration preview via Pillow."""
        import math
        from PIL import Image, ImageDraw, ImageFont

        S = 2
        W, H = max(120, tw * S), max(80, th * S)
        slug = str(t_meta.get("slug") or t_meta.get("art") or "")
        is_dark = bool(t_meta.get("is_dark", False))

        def _hex_rgba(hx, a=255):
            hx = (hx or "#888888").lstrip("#")
            if len(hx) == 6:
                return (int(hx[0:2], 16), int(hx[2:4], 16), int(hx[4:6], 16), a)
            return (128, 128, 128, a)

        bg_rgba = _hex_rgba(t_meta.get("bg", "#ffffff"), 255)
        acc_rgba = _hex_rgba(t_meta.get("accent", "#2563eb"), 255)
        img = Image.new("RGBA", (W, H), bg_rgba)
        d = ImageDraw.Draw(img, "RGBA")

        # Card surface inset
        pad = int(10 * S)
        cx0, cy0, cx1, cy1 = pad, int(14 * S), W - pad, H - int(14 * S)
        card_bg = (30, 34, 52, 245) if is_dark else (255, 255, 255, 245)
        if slug == "crane_cloud":
            card_bg = (252, 249, 242, 252)
        elif slug == "bamboo_panda":
            card_bg = (244, 250, 243, 252)
        elif slug == "amber_cat":
            card_bg = (254, 249, 240, 252)
        elif slug == "camellia_flora":
            card_bg = (254, 247, 248, 252)
        elif slug == "starry_fox":
            card_bg = (22, 25, 46, 252)
        elif slug == "forest_stag":
            card_bg = (18, 32, 28, 252)

        d.rounded_rectangle([cx0, cy0, cx1, cy1], radius=int(12 * S), fill=card_bg, outline=(acc_rgba[0], acc_rgba[1], acc_rgba[2], 115), width=max(2, S))

        # Draw artistic botanical / fauna illustration on the right side of the card
        rx = cx1 - int(44 * S)
        ry = (cy0 + cy1) // 2
        if slug == "crane_cloud":
            # Vermilion sun + golden clouds + soaring crane silhouette
            sr = int(20 * S)
            d.ellipse([rx - sr, cy0 + int(10 * S), rx + sr, cy0 + int(10 * S) + sr * 2], fill=(206, 58, 40, 68))
            d.rounded_rectangle([rx - int(34 * S), cy0 + int(34 * S), rx + int(6 * S), cy0 + int(44 * S)], radius=int(5 * S), fill=(198, 148, 72, 85))
            d.rounded_rectangle([rx - int(10 * S), cy0 + int(40 * S), rx + int(28 * S), cy0 + int(49 * S)], radius=int(5 * S), fill=(198, 148, 72, 75))
            # Crane wings
            d.line([(rx - int(26 * S), cy0 + int(28 * S)), (rx - int(4 * S), cy0 + int(16 * S)), (rx + int(18 * S), cy0 + int(23 * S))], fill=(48, 40, 36, 165), width=int(2.4 * S))
            d.line([(rx - int(14 * S), cy0 + int(25 * S)), (rx + int(8 * S), cy0 + int(33 * S))], fill=(48, 40, 36, 135), width=int(1.8 * S))
        elif slug == "bamboo_panda":
            # Bamboo stalks, leaves & cute minimalist ink panda
            d.line([(rx - int(20 * S), cy1 - int(4 * S)), (rx - int(6 * S), cy0 + int(6 * S))], fill=(42, 124, 78, 110), width=int(3.0 * S))
            d.polygon([(rx - int(10 * S), ry - int(8 * S)), (rx - int(32 * S), ry + int(2 * S)), (rx - int(18 * S), ry + int(6 * S))], fill=(42, 124, 78, 95))
            d.polygon([(rx - int(8 * S), ry - int(14 * S)), (rx + int(14 * S), ry - int(4 * S)), (rx + int(2 * S), ry + int(2 * S))], fill=(42, 124, 78, 95))
            # Panda head & ears in bottom-right
            px, py = rx + int(12 * S), cy1 - int(22 * S)
            er = int(5.5 * S)
            hr = int(13 * S)
            d.ellipse([px - int(11 * S) - er, py - int(9 * S) - er, px - int(11 * S) + er, py - int(9 * S) + er], fill=(36, 44, 38, 185))
            d.ellipse([px + int(11 * S) - er, py - int(9 * S) - er, px + int(11 * S) + er, py - int(9 * S) + er], fill=(36, 44, 38, 185))
            d.ellipse([px - hr, py - hr, px + hr, py + hr], fill=(252, 254, 252, 235), outline=(42, 124, 78, 150), width=int(1.8 * S))
            d.ellipse([px - int(7 * S), py - int(3 * S), px - int(2 * S), py + int(2 * S)], fill=(36, 44, 38, 195))
            d.ellipse([px + int(2 * S), py - int(3 * S), px + int(7 * S), py + int(2 * S)], fill=(36, 44, 38, 195))
        elif slug == "amber_cat":
            # Paw prints + sleeping amber cat with pointed ears
            for px_p, py_p, sc_p in ((rx - int(26 * S), cy0 + int(22 * S), 1.0), (rx - int(4 * S), cy0 + int(34 * S), 0.85)):
                pr = int(5.2 * S * sc_p)
                d.ellipse([px_p - pr, py_p - pr, px_p + pr, py_p + pr], fill=(208, 102, 34, 95))
                for dx_t, dy_t in ((-5.5, -6.5), (0.0, -8.0), (5.5, -6.5)):
                    tx_p = px_p + int(dx_t * S * sc_p)
                    ty_p = py_p + int(dy_t * S * sc_p)
                    tr = int(2.4 * S * sc_p)
                    d.ellipse([tx_p - tr, ty_p - tr, tx_p + tr, ty_p + tr], fill=(208, 102, 34, 95))
            cx_c, cy_c = rx + int(12 * S), cy1 - int(20 * S)
            cr = int(13 * S)
            d.polygon([(cx_c - int(11 * S), cy_c - int(4 * S)), (cx_c - int(8 * S), cy_c - int(19 * S)), (cx_c - int(1 * S), cy_c - int(9 * S))], fill=(208, 102, 34, 150))
            d.polygon([(cx_c + int(11 * S), cy_c - int(4 * S)), (cx_c + int(8 * S), cy_c - int(19 * S)), (cx_c + int(1 * S), cy_c - int(9 * S))], fill=(208, 102, 34, 150))
            d.ellipse([cx_c - cr, cy_c - cr + int(2 * S), cx_c + cr, cy_c + cr], fill=(234, 138, 66, 135))
        elif slug == "camellia_flora":
            # 5-petal botanical camellia flower + leaves
            fcx, fcy = rx + int(4 * S), cy0 + int(32 * S)
            d.polygon([(fcx - int(10 * S), fcy + int(8 * S)), (fcx - int(32 * S), fcy + int(20 * S)), (fcx - int(14 * S), fcy + int(24 * S))], fill=(56, 118, 92, 115))
            for i in range(5):
                ang = math.radians(i * 72 - 18)
                px_f = fcx + int(11 * S * math.cos(ang))
                py_f = fcy + int(11 * S * math.sin(ang))
                pr = int(10 * S)
                d.ellipse([px_f - pr, py_f - pr, px_f + pr, py_f + pr], fill=(196, 68, 92, 88), outline=(184, 58, 82, 120), width=S)
            d.ellipse([fcx - int(5 * S), fcy - int(5 * S), fcx + int(5 * S), fcy + int(5 * S)], fill=(224, 168, 52, 195))
        elif slug == "starry_fox":
            # Golden crescent moon + twinkling stars + fox ears silhouette
            mcx, mcy = rx + int(8 * S), cy0 + int(26 * S)
            mr = int(14 * S)
            d.ellipse([mcx - mr, mcy - mr, mcx + mr, mcy + mr], fill=(246, 198, 96, 145))
            d.ellipse([mcx - mr + int(7 * S), mcy - mr - int(4 * S), mcx + mr + int(7 * S), mcy + mr - int(4 * S)], fill=card_bg)
            for sx, sy, sr in ((rx - int(22 * S), cy0 + int(22 * S), 4.5), (rx - int(8 * S), cy0 + int(42 * S), 3.2)):
                r_s = int(sr * S)
                d.polygon([(sx, sy - r_s), (sx + r_s // 3, sy - r_s // 3), (sx + r_s, sy), (sx + r_s // 3, sy + r_s // 3),
                           (sx, sy + r_s), (sx - r_s // 3, sy + r_s // 3), (sx - r_s, sy), (sx - r_s // 3, sy - r_s // 3)], fill=(255, 228, 150, 185))
        elif slug == "forest_stag":
            # Stag antlers + glowing fireflies
            acx, acy = rx + int(4 * S), cy1 - int(8 * S)
            d.line([(acx, acy), (acx - int(14 * S), acy - int(32 * S))], fill=(112, 218, 166, 135), width=int(2.4 * S))
            d.line([(acx - int(8 * S), acy - int(18 * S)), (acx - int(22 * S), acy - int(24 * S))], fill=(112, 218, 166, 125), width=int(2.0 * S))
            d.line([(acx + int(6 * S), acy), (acx + int(20 * S), acy - int(32 * S))], fill=(112, 218, 166, 135), width=int(2.4 * S))
            for ffx, ffy in ((rx - int(24 * S), cy0 + int(24 * S)), (rx + int(4 * S), cy0 + int(18 * S))):
                d.ellipse([ffx - int(4 * S), ffy - int(4 * S), ffx + int(4 * S), ffy + int(4 * S)], fill=(242, 208, 112, 195))

        # Draw simulated candidate HUD text on the left side
        try:
            f_code = ImageFont.truetype("C:/Windows/Fonts/consolab.ttf", int(12 * S))
            f_cand = ImageFont.truetype("C:/Windows/Fonts/msyhbd.ttc", int(14 * S))
            f_rest = ImageFont.truetype("C:/Windows/Fonts/msyh.ttc", int(13 * S))
        except Exception:
            f_code = f_cand = f_rest = ImageFont.load_default()

        tx = cx0 + int(14 * S)
        d.text((tx, cy0 + int(12 * S)), "heyu", font=f_code, fill=acc_rgba)
        pill_y0 = cy0 + int(34 * S)
        pill_y1 = pill_y0 + int(26 * S)
        d.rounded_rectangle([tx, pill_y0, tx + int(64 * S), pill_y1], radius=int(6 * S), fill=(acc_rgba[0], acc_rgba[1], acc_rgba[2], 48 if not is_dark else 72))
        txt_fg = (242, 244, 250, 255) if is_dark else (34, 36, 42, 255)
        d.text((tx + int(7 * S), pill_y0 + int(3 * S)), "1 鹤羽", font=f_cand, fill=acc_rgba if not is_dark else (255, 238, 198, 255))
        d.text((tx + int(74 * S), pill_y0 + int(4 * S)), "2 荷雨  3 合语", font=f_rest, fill=txt_fg)

        return img.resize((tw, th), Image.LANCZOS)

    def _load_thumbs_async(self):
        sc = self._ws()
        tw, th = int((self.THUMB_W - 4) * sc), int((self.THUMB_H - 8) * sc)
        items = [dict(t) for t in self.theme_meta]

        def work():
            try:
                from PIL import Image
            except Exception:
                return None
            out = {}
            for t_item in items:
                idx = t_item["index"]
                fname = t_item.get("file")
                is_art = bool(idx >= 9 or t_item.get("art"))
                path = os.path.join(PREVIEW_DIR, fname) if (fname and not is_art) else None
                if path and os.path.exists(path):
                    try:
                        img = Image.open(path)
                        img.load()
                        img = img.convert("RGBA")
                        w, h = img.size
                        s = min(tw / w, th / h)
                        size = (max(1, int(w * s)), max(1, int(h * s)))
                        out[idx] = img.resize(size, Image.LANCZOS)
                        continue
                    except Exception as e:
                        print(f"[studio] preview load failed {path}: {e}", file=sys.stderr)
                try:
                    out[idx] = HeyuStudioApp._render_illustrated_theme_preview(t_item, tw, th)
                except Exception as e:
                    print(f"[studio] synth preview failed {idx}: {e}", file=sys.stderr)
            return out

        def done(res):
            if not res:
                return
            self._gallery_images = getattr(self, "_gallery_images", [])
            for idx, img in res.items():
                lbl = self._gallery_thumbs.get(idx)
                if lbl is None:
                    continue
                cimg = ctk.CTkImage(light_image=img, dark_image=img, size=(img.size[0] / sc, img.size[1] / sc))
                self._gallery_images.append(cimg)
                try:
                    lbl.configure(image=cimg)
                except Exception:
                    pass

        self._run_bg(work, done)

    def _refresh_theme_cards(self):
        cur = self.cfg.get("theme_index", 0)
        for idx, (card, check) in self._gallery_cards.items():
            sel = idx == cur
            card.configure(border_color=C("accent") if sel else C("border"), border_width=2 if sel else 1)
            if sel and not check.winfo_ismapped():
                check.pack(side="right")
            elif not sel and check.winfo_ismapped():
                check.pack_forget()

    def _set_theme(self, theme_idx: int):
        self._set_cfg("theme_index", theme_idx)
        self._refresh_theme_cards()
        self._update_live_hud_preview()

    # =========================================================================
    # PAGE ⑤ 安全与隐私 SECURITY & PRIVACY (独立核心安全专栏 · 最高优先级)
    # =========================================================================
    def _build_page_security(self):
        page, col = self._page(
            "安全与隐私",
            "安全与数据隐私保护始终放在第一位 · 顶级密文加密 · 默认用完即毁零残留 · 设备硬件绑定",
        )

        # ---- 1. Master Password & Cryptographic Vault Card
        g_vault = self._group(
            col,
            "🔐 顶级密文保险箱与主密码保护（PBKDF2 120,000 轮 + 256-bit 认证加密）",
            note="HEYUENC1 密文落盘 · 换机强制重登",
            pady_top=14,
        )
        r_mp = self._row(
            g_vault,
            "安全主密码与密码学强度",
            "强制要求 ≥10 位且包含大写、小写、数字、特殊符号中的至少 3 类（高信息熵抗暴力破解）。\n"
            "• 新增词条免密直添；查看词库全部词条必须验证主密码；\n"
            "• 更换电脑设备后硬件指纹改变，必须重新输入主密码登录授权。",
            wrap=480,
            badge="防暴力破解",
        )
        mp_btns = ctk.CTkFrame(r_mp.slot, fg_color="transparent")
        mp_btns.pack()
        self.sec_pwd_status_lbl = self._label(mp_btns, "", 12, "bold")
        self.sec_pwd_status_lbl.pack(side="left", padx=(0, 10))
        self._btn(
            mp_btns,
            "🔑 设置 / 修改主密码",
            lambda: self._open_master_setup_modal(mandatory=False),
            kind="primary",
            height=32,
        ).pack(side="left")

        r_rem = self._row(
            g_vault,
            "定期防遗忘验证 & 词库加密备份提醒",
            "每两周（14 天）强制提醒输入一次主密码防遗忘；每月（30 天）提醒导出保存最新加密词库备份。",
            wrap=680,
            stacked=True,
            badge="双重守护",
        )
        rem_btns = ctk.CTkFrame(r_rem.slot, fg_color="transparent")
        rem_btns.pack(fill="x")
        self.sec_cycle_lbl = self._label(rem_btns, "", 12, "bold", color="accent")
        self.sec_cycle_lbl.pack(side="left", padx=(0, 14))
        self._btn(rem_btns, "💾 导出最新加密词库备份 (.heyu-vault)", self._export_encrypted_lexicon_backup, kind="primary", height=32).pack(side="right")
        self._btn(rem_btns, "✅ 立即验证主密码（重置 14 天周期）", self._open_periodic_verify_modal, height=32).pack(side="right", padx=(0, 8))

        # ---- 2. Status Pill Identity (Username / Avatar / Avatar Size)
        g_id = self._group(
            col,
            "👤 悬浮状态胶囊身份定制（用户名 / 自定义头像 / 尺寸控制）",
            note="自定义用户名或上传头像取代胶囊上的「鹤」字，留空则默认显示「鹤」字",
        )
        r_un = self._row(g_id, "胶囊显示用户名", "设置后将取代状态胶囊上的「鹤」字；若留空则默认显示「鹤」字")
        un_box = ctk.CTkFrame(r_un.slot, fg_color="transparent")
        un_box.pack()
        self.sec_uname_entry = self._entry(un_box, 160, "留空默认显示「鹤」")
        cur_uname = str(self.cfg.get("user_name", "")).strip()
        if cur_uname:
            self.sec_uname_entry.insert(0, cur_uname)
        self.sec_uname_entry.pack(side="left", padx=(0, 8))
        self.sec_uname_entry.bind("<Return>", lambda e: self._save_pill_identity_from_ui(self.sec_uname_entry))
        self._btn(
            un_box,
            "保存用户名",
            lambda: self._save_pill_identity_from_ui(self.sec_uname_entry),
            kind="primary",
            height=30,
        ).pack(side="left")

        r_av = self._row(g_id, "胶囊自定义头像", "上传自定义头像取代胶囊上的「鹤」字，引擎实时渲染抗锯齿圆形头像")
        av_box = ctk.CTkFrame(r_av.slot, fg_color="transparent")
        av_box.pack()
        av_on = bool(self.cfg.get("avatar_enabled", False))
        self.sec_av_status_lbl = self._label(
            av_box,
            "✓ 已启用自定义头像" if av_on else "当前显示文字徽章",
            12,
            "bold",
            color="success" if av_on else "muted",
        )
        self.sec_av_status_lbl.pack(side="left", padx=(0, 10))
        self._btn(av_box, "🖼 上传头像…", self._upload_pill_avatar_dialog, kind="primary", height=30).pack(side="left", padx=(0, 6))
        self._btn(av_box, "恢复文字「鹤」", self._clear_pill_avatar, height=30).pack(side="left")

        r_asz = self._row(g_id, "头像在胶囊上的显示大小", "精细调节头像在状态胶囊上的显示直径（16px ～ 36px，默认 26px）")
        av_sz = max(16, min(36, int(self.cfg.get("avatar_size", 26))))
        self.lbl_sec_av_sz = self._label(r_asz.slot, f"{av_sz} px", 13, "bold", color="accent", mono=True, width=54)
        self.sec_av_slider = ctk.CTkSlider(
            r_asz.slot, from_=16, to=36, number_of_steps=20, width=180, height=18,
            fg_color=C("seg_bg"), progress_color=C("accent"), button_color=C("accent"),
            button_hover_color=C("accent_hover"), command=self._on_avatar_size_slider,
        )
        self.sec_av_slider.set(av_sz)
        self.sec_av_slider.pack(side="left")
        self.lbl_sec_av_sz.pack(side="left", padx=(10, 0))

        # ---- 3. Default Burn-After-Use (用完即毁) Data Privacy Controls
        hist_path = os.path.join(DATA_DIR, "input_history.enc")
        g_hist = self._group(
            col,
            "🔥 默认「用完即毁」零残留策略（打字历史与音频数据）",
            note="除非您手动开启记录或点击录音，否则软件默认不保留任何打字历史与语音音频，用完即毁最安全",
        )
        r_aud_priv = self._row(
            g_hist,
            "语音转写音频用完即毁",
            "实时语音听写与电脑声音转写全程 100% 在内存（RAM）中完成，识别结束瞬间以全零字节覆写内存缓冲区，绝不落盘保留任何音频文件（除非您在 48kHz 录音棚主动点击录音）。",
            wrap=520,
            badge="内存全零销毁",
        )
        self._chip(r_aud_priv.slot, "✓ 默认开启 · 用完即毁", color="success", mono=False, size=12).pack()

        r = self._row(
            g_hist,
            "打字历史记录策略",
            "默认「关闭 · 用完即毁（不记录任何打字历史）」。仅在您手动选择保留时长后，才会以 PBKDF2 密文暂存并在到期后自动全零覆写销毁。",
            stacked=True,
            badge="默认不记录",
        )
        ret_opts = [
            ("关闭 · 用完即毁 (默认推荐)", 0),
            ("15 分钟", 15),
            ("30 分钟", 30),
            ("1 小时", 60),
            ("2 小时", 120),
            ("24 小时", 1440),
        ]
        cur_ret = int(self.cfg.get("history_retention_mins", 0))
        if cur_ret not in [v for _, v in ret_opts]:
            cur_ret = 0
        self._choice(r.slot, ret_opts, cur_ret, self._on_history_retention_change).pack(anchor="w")

        r = self._row(g_hist, "最大保留容量", "手动开启历史记录时，超过条数上限将自动淘汰最早记录")
        max_opts = [("50 条", 50), ("100 条", 100), ("200 条", 200), ("500 条", 500)]
        cur_max = int(self.cfg.get("history_max_items", 200))
        if cur_max not in [v for _, v in max_opts]:
            cur_max = 200
        self._choice(r.slot, max_opts, cur_max, self._on_history_max_change).pack()

        r = self._row(g_hist, "独立 PIN 码加密锁", f"加密保险箱路径：{hist_path}")
        pin_bar = ctk.CTkFrame(r.slot, fg_color="transparent")
        pin_bar.pack()
        self.pin_status_lbl = self._label(pin_bar, "", 12, "bold")
        self.pin_status_lbl.pack(side="left", padx=(0, 10))
        self.pin_unlock_btn = self._btn(pin_bar, "🔓 解锁查看", self._unlock_pin_dialog, height=30)
        self.pin_set_btn = self._btn(pin_bar, "🔑 设置 PIN 码", self._open_pin_dialog, height=30)
        self.pin_set_btn.pack(side="left")

        self._hairline(g_hist).pack(fill="x", padx=(int(ROW_PX * self._ws()), 0))
        hbar = ctk.CTkFrame(g_hist, fg_color="transparent")
        hbar.pack(fill="x", padx=ROW_PX, pady=(12, 6))
        self.hist_count_lbl = self._label(hbar, "最近临时记录（倒序）", 13, "bold")
        self.hist_count_lbl.pack(side="left")
        self._btn(hbar, "🔥 立即全零覆写销毁历史", self._wipe_history_now, kind="danger", height=30).pack(side="right")
        self._btn(hbar, "🔄 刷新", self._refresh_history_box, height=30).pack(side="right", padx=(0, 8))
        self._btn(hbar, "📋 一键合并复制全部", self._copy_all_history, kind="primary", height=30).pack(side="right", padx=(0, 8))

        self.history_box = self._textbox(g_hist, mono=False, size=13, height=110)
        self.history_box.pack(fill="x", padx=ROW_PX, pady=(2, 16))
        self._refresh_history_box()
        self._refresh_security_page_status()
        return page

    def _on_show_security(self):
        self._refresh_security_page_status()
        self._refresh_history_box()

    def _refresh_security_page_status(self):
        has_mp = self._has_master_password()
        st, _ = _resolve_authorized_lexicon_key_py(DATA_DIR, self.cfg)
        if hasattr(self, "sec_pwd_status_lbl"):
            if has_mp and st == "ok":
                self.sec_pwd_status_lbl.configure(text="✓ 主密码已启用 · 本机硬件已授权", text_color=C("success"))
            elif has_mp and st == "new_device":
                self.sec_pwd_status_lbl.configure(text="⚠ 检测到新设备 · 需验证主密码", text_color=C("danger"))
            else:
                self.sec_pwd_status_lbl.configure(text="⚠ 尚未设置安全主密码", text_color=C("danger"))
        if hasattr(self, "sec_cycle_lbl"):
            now = int(time.time())
            last_v = int(self.cfg.get("last_password_verified_ts", 0)) or now
            last_b = int(self.cfg.get("last_lexicon_backup_ts", 0)) or now
            rem_v_days = max(0, 14 - int((now - last_v) // 86400))
            rem_b_days = max(0, 30 - int((now - last_b) // 86400))
            self.sec_cycle_lbl.configure(
                text=f"距下次密码验证：{rem_v_days} 天 · 距下次备份提醒：{rem_b_days} 天"
            )

    def _export_encrypted_lexicon_backup(self):
        from tkinter import filedialog
        headers, entries = self._read_user_dict_entries()
        self._write_user_dict_entries(headers, entries)
        src_p = os.path.join(DATA_DIR, "user_custom.tsv")
        raw_cipher = ""
        if os.path.exists(src_p):
            with open(src_p, "r", encoding="utf-8") as f:
                raw_cipher = f.read()
        dst = filedialog.asksaveasfilename(
            parent=self,
            title="导出保存最新加密词库备份（HEYUENC1 顶级密文）",
            initialfile=f"鹤羽加密词库备份_{time.strftime('%Y%m%d')}.heyu-vault",
            defaultextension=".heyu-vault",
            filetypes=[("鹤羽加密词库保险箱", "*.heyu-vault"), ("TSV 密文文件", "*.tsv"), ("所有文件", "*.*")],
        )
        if not dst:
            return
        backup_obj = {
            "format": "HEYU_ENCRYPTED_LEXICON_BACKUP_V1",
            "exported_at": time.strftime("%Y-%m-%d %H:%M:%S"),
            "master_pwd_salt": self.cfg.get("master_pwd_salt", ""),
            "master_pwd_verifier": self.cfg.get("master_pwd_verifier", ""),
            "master_wrapped_vault_key": self.cfg.get("master_wrapped_vault_key", ""),
            "ciphertext": raw_cipher.strip(),
        }
        _write_json(dst, backup_obj)
        self.cfg["last_lexicon_backup_ts"] = int(time.time())
        self._cfg_touched.add("last_lexicon_backup_ts")
        self._save_config()
        self._refresh_security_page_status()
        messagebox.showinfo(
            "加密词库备份成功",
            f"已将最新词库以 HEYUENC1 (256-bit) 认证密文导出至：\n{dst}\n\n"
            "已重置 30 天词库备份提醒周期。",
            parent=self,
        )

    # =========================================================================
    # PRE-INSTALL / FIRST-RUN SETUP WIZARD & PERIODIC SECURITY GATES
    # =========================================================================
    def _build_setup_wizard_shell(self):
        """Standalone `--setup-wizard` UI launched before/during install or first run."""
        self._wiz_step = 1
        self._wiz_pending_pwd = ""
        root = ctk.CTkFrame(self, fg_color=C("bg"))
        root.pack(fill="both", expand=True, padx=28, pady=22)
        self._populate_master_setup_ui(root, is_standalone_wizard=True)

    def _populate_master_setup_ui(self, container, is_standalone_wizard=False, modal_win=None):
        for w in container.winfo_children():
            w.destroy()

        # Header
        hdr = self._card(container)
        hdr.pack(fill="x", pady=(0, 14))
        hi = ctk.CTkFrame(hdr, fg_color="transparent")
        hi.pack(fill="x", padx=22, pady=16)
        ctk.CTkLabel(
            hi, text="🛡️", width=48, height=48, corner_radius=14,
            fg_color=C("accent"), text_color=C("on_accent"), font=self.F(22, "bold"),
        ).pack(side="left")
        ht = ctk.CTkFrame(hi, fg_color="transparent")
        ht.pack(side="left", padx=(14, 0))
        self._label(ht, "鹤羽音形 · 安全主密码与胶囊个性化初始化", 18, "bold").pack(anchor="w")
        self._label(
            ht,
            "安全与数据隐私保护始终放在第一位 · 所有个人词库与用户数据均采用 PBKDF2 + 256-bit 顶级密文加密",
            12,
            color="text_2",
        ).pack(anchor="w", pady=(3, 0))

        # Body Host (switches between Step 1: Scheme + Password & Identity -> Step 2: Mandatory Re-entry Confirmation)
        body = self._card(container)
        body.pack(fill="both", expand=True)
        step1_frame = ctk.CTkFrame(body, fg_color="transparent")
        step1_frame.pack(fill="both", expand=True, padx=24, pady=14)

        self._label(
            step1_frame,
            "🎯 首选输入方案：您想使用【全拼】、【双拼】还是【鹤羽音形】输入？",
            15,
            "bold",
            color="accent",
        ).pack(anchor="w")
        self._label(
            step1_frame,
            "安装后可随时在「设置 → 常用」顶部一键切换。选择全拼也能通过整句 Viterbi 流式解码与超级简拼轻松达到 120+ 字/分：",
            12,
            color="text_2",
            wraplength=680,
        ).pack(anchor="w", pady=(2, 8))

        wiz_scheme_row = ctk.CTkFrame(step1_frame, fg_color="transparent")
        wiz_scheme_row.pack(fill="x", pady=(0, 8))
        wiz_scheme_seg = self._choice(
            wiz_scheme_row,
            [
                ("🚀 智能全拼 (整句流式 · 120+字/分)", "quanpin"),
                ("⚡ 鹤羽双拼 (一字两键 · 150+字/分)", "shuangpin"),
                ("🦅 鹤羽音形 (四码盲打 · 200+字/分)", "yinxing"),
            ],
            self.cfg.get("input_scheme", "yinxing"),
            lambda k: self._select_input_scheme(k),
        )
        wiz_scheme_seg.pack(anchor="w")

        self._hairline(step1_frame).pack(fill="x", pady=8)

        self._label(step1_frame, "🔐 第一步：设置高强度安全主密码（防暴力破解）", 15, "bold", color="accent").pack(anchor="w")
        self._label(
            step1_frame,
            "密码学安全要求：长度至少 10 位，且包含【大写字母、小写字母、数字、特殊符号】中的至少 3 类，高信息熵防暴力破解。\n"
            "• 日常打字与新增词条无需输入密码；查看词库全部词条或更换电脑设备时必须验证主密码。",
            12,
            color="text_2",
            wraplength=650,
        ).pack(anchor="w", pady=(4, 10))

        pw_grid = ctk.CTkFrame(step1_frame, fg_color="transparent")
        pw_grid.pack(fill="x", pady=(0, 6))
        pw_grid.grid_columnconfigure(1, weight=1)

        cur_row = 0
        old_pwd_entry = None
        if self._has_master_password():
            self._label(pw_grid, "当前主密码", 13, "bold").grid(row=cur_row, column=0, sticky="w", pady=6, padx=(0, 12))
            old_pwd_entry = self._entry(pw_grid, 320, "请输入当前主密码进行验证")
            old_pwd_entry.configure(show="•")
            old_pwd_entry.grid(row=cur_row, column=1, sticky="ew", pady=6)
            cur_row += 1

        self._label(pw_grid, "设置新主密码", 13, "bold").grid(row=cur_row, column=0, sticky="w", pady=6, padx=(0, 12))
        pw1_entry = self._entry(pw_grid, 320, "至少 10 位，含大小写字母/数字/符号中至少 3 类")
        pw1_entry.configure(show="•")
        pw1_entry.grid(row=cur_row, column=1, sticky="ew", pady=6)
        cur_row += 1

        strength_lbl = self._label(step1_frame, "密码强度：等待输入…", 12, "bold", color="muted")
        strength_lbl.pack(anchor="w", pady=(2, 10))

        def _on_pw1_key(_e=None):
            ok, msg, _sc = _check_password_strength(pw1_entry.get())
            strength_lbl.configure(text=msg, text_color=C("success") if ok else C("danger"))

        pw1_entry.bind("<KeyRelease>", _on_pw1_key)

        self._hairline(step1_frame).pack(fill="x", pady=10)
        self._label(step1_frame, "个性化选填：设置胶囊用户名或上传自定义头像（取代默认「鹤」字）", 15, "bold", color="accent").pack(anchor="w")
        self._label(
            step1_frame,
            "您可以给自己起一个用户名直接显示在桌面状态胶囊上（不设置则默认显示「鹤」字），或上传自定义头像并调节显示大小。",
            12,
            color="text_2",
            wraplength=650,
        ).pack(anchor="w", pady=(4, 10))

        id_grid = ctk.CTkFrame(step1_frame, fg_color="transparent")
        id_grid.pack(fill="x")
        id_grid.grid_columnconfigure(1, weight=1)

        self._label(id_grid, "胶囊用户名（选填）", 13, "bold").grid(row=0, column=0, sticky="w", pady=6, padx=(0, 12))
        uname_entry = self._entry(id_grid, 220, "留空则默认显示「鹤」字")
        if str(self.cfg.get("user_name", "")).strip():
            uname_entry.insert(0, str(self.cfg.get("user_name", "")).strip())
        uname_entry.grid(row=0, column=1, sticky="w", pady=6)

        self._label(id_grid, "自定义头像（选填）", 13, "bold").grid(row=1, column=0, sticky="w", pady=6, padx=(0, 12))
        av_row = ctk.CTkFrame(id_grid, fg_color="transparent")
        av_row.grid(row=1, column=1, sticky="w", pady=6)
        av_on = bool(self.cfg.get("avatar_enabled", False))
        self.wiz_av_status_lbl = self._label(
            av_row,
            "✓ 已启用自定义头像" if av_on else "默认显示文字徽章",
            12,
            "bold",
            color="success" if av_on else "muted",
        )
        self.wiz_av_status_lbl.pack(side="left", padx=(0, 10))
        self._btn(av_row, "🖼 选择头像图片…", self._upload_pill_avatar_dialog, height=28).pack(side="left", padx=(0, 6))
        self._btn(av_row, "使用文字", self._clear_pill_avatar, height=28).pack(side="left")

        self._label(id_grid, "头像显示大小", 13, "bold").grid(row=2, column=0, sticky="w", pady=6, padx=(0, 12))
        sz_row = ctk.CTkFrame(id_grid, fg_color="transparent")
        sz_row.grid(row=2, column=1, sticky="w", pady=6)
        init_sz = max(16, min(36, int(self.cfg.get("avatar_size", 26))))
        sz_val_lbl = self._label(sz_row, f"{init_sz} px", 12, "bold", color="accent", mono=True, width=50)
        sz_slider = ctk.CTkSlider(
            sz_row, from_=16, to=36, number_of_steps=20, width=180, height=18,
            fg_color=C("seg_bg"), progress_color=C("accent"), button_color=C("accent"),
            command=lambda v: (sz_val_lbl.configure(text=f"{int(round(float(v)))} px"), self._on_avatar_size_slider(v)),
        )
        sz_slider.set(init_sz)
        sz_slider.pack(side="left")
        sz_val_lbl.pack(side="left", padx=(8, 0))

        # Step 2 Frame (Mandatory Password Save Reminder & Re-entry Verification)
        step2_frame = ctk.CTkFrame(body, fg_color="transparent")
        self._label(step2_frame, "⚠️ 第二步：请妥善保存您的密码，并再次输入一遍以防万一", 16, "bold", color="accent").pack(anchor="w")
        self._label(
            step2_frame,
            "重要安全提醒：\n"
            "1. 您的词库与用户数据将使用该主密码进行 PBKDF2 + 256-bit 顶级密文加密，软件不设任何后门，一旦遗忘密码将无法查看加密词库。\n"
            "2. 请现在将主密码记录在您的密码管理器或安全笔记本中妥善保存！\n"
            "3. 软件每两周（14 天）会提醒您验证一次密码以防遗忘，每月（30 天）会提醒您导出备份最新词库。\n\n"
            "请在下方再次完整输入一遍您刚才设置的主密码以完成最终验证：",
            13,
            color="text_2",
            wraplength=650,
        ).pack(anchor="w", pady=(10, 16))

        pw2_entry = self._entry(step2_frame, 360, "请再次输入一遍您的安全主密码进行确认…")
        pw2_entry.configure(show="•")
        pw2_entry.pack(anchor="w", pady=(0, 8))
        step2_err_lbl = self._label(step2_frame, "", 12, "bold", color="danger")
        step2_err_lbl.pack(anchor="w", pady=(4, 12))

        def _go_to_step2():
            existing_vk = None
            if old_pwd_entry is not None:
                ok_old, vk_old = _master_verify_password(DATA_DIR, old_pwd_entry.get(), self.cfg)
                if not ok_old:
                    strength_lbl.configure(text="❌ 当前主密码验证错误，请重新输入。", text_color=C("danger"))
                    return
                existing_vk = vk_old
            cand_pwd = pw1_entry.get()
            ok_str, msg_str, _ = _check_password_strength(cand_pwd)
            if not ok_str:
                strength_lbl.configure(text=f"❌ {msg_str}", text_color=C("danger"))
                return
            self._wiz_pending_pwd = cand_pwd
            self._wiz_existing_vk = existing_vk
            self._wiz_pending_uname = uname_entry.get().strip()[:16]
            step1_frame.pack_forget()
            step2_frame.pack(fill="both", expand=True, padx=24, pady=22)
            pw2_entry.delete(0, "end")
            pw2_entry.focus_set()

        def _finish_step2(_e=None):
            re_pwd = pw2_entry.get()
            if re_pwd != self._wiz_pending_pwd:
                step2_err_lbl.configure(text="❌ 两次输入的密码不一致！请核对您保存的密码后重新输入。")
                return
            # Create or update cryptographic master vault
            headers, entries = self._read_user_dict_entries()
            params, vkey = _master_create_or_update_vault(
                DATA_DIR, self._wiz_pending_pwd, existing_vault_key=getattr(self, "_wiz_existing_vk", None)
            )
            self._unlocked_vault_key = vkey
            self._dict_unlocked = True
            for k, val in params.items():
                self.cfg[k] = val
                self._cfg_touched.add(k)
            self.cfg["user_name"] = getattr(self, "_wiz_pending_uname", "")
            self._cfg_touched.add("user_name")
            self._save_config()
            # Encrypt user_custom.tsv immediately with the 256-bit vault key
            self._write_user_dict_entries(headers, entries)
            self._refresh_security_page_status()
            self._refresh_dict_lock_view()
            messagebox.showinfo(
                "安全初始化完成",
                "✓ 安全主密码与顶级密文保险箱已生效！\n"
                "✓ 个人词库已使用 HEYUENC1 (256-bit) 认证密文加密保护。\n"
                "✓ 请务必妥善保管好您的主密码。",
                parent=modal_win or self,
            )
            if modal_win is not None:
                modal_win.destroy()
            elif is_standalone_wizard:
                self.destroy()

        pw2_entry.bind("<Return>", _finish_step2)

        btn_bar1 = ctk.CTkFrame(step1_frame, fg_color="transparent")
        btn_bar1.pack(fill="x", pady=(14, 0))
        self._btn(
            btn_bar1,
            "下一步：妥善保存密码并进行二次确认 ▸",
            _go_to_step2,
            kind="primary",
            height=38,
        ).pack(side="right")

        btn_bar2 = ctk.CTkFrame(step2_frame, fg_color="transparent")
        btn_bar2.pack(fill="x", pady=(12, 0))
        self._btn(
            btn_bar2,
            "◂ 返回上一步修改",
            lambda: (step2_frame.pack_forget(), step1_frame.pack(fill="both", expand=True, padx=24, pady=18)),
            height=36,
        ).pack(side="left")
        self._btn(
            btn_bar2,
            "✅ 确认无误 · 启用顶级密文加密保护",
            _finish_step2,
            kind="primary",
            height=38,
        ).pack(side="right")

    def _open_master_setup_modal(self, mandatory=False):
        if getattr(self, "_active_sec_modal", None) is not None:
            try:
                if self._active_sec_modal.winfo_exists():
                    self._active_sec_modal.lift()
                    return
            except Exception:
                pass
        win = ctk.CTkToplevel(self)
        self._active_sec_modal = win
        win.title("鹤羽音形 · 安全主密码与胶囊身份设置")
        win.geometry("740x690")
        win.minsize(700, 650)
        win.configure(fg_color=C("bg"))
        win.transient(self)
        win.grab_set()
        if mandatory:
            win.protocol("WM_DELETE_WINDOW", lambda: messagebox.showwarning(
                "必须设置安全主密码",
                "为保障您的词库与隐私数据绝对安全，首次使用必须完成安全主密码设置与二次确认。",
                parent=win,
            ))
        host = ctk.CTkFrame(win, fg_color=C("bg"))
        host.pack(fill="both", expand=True, padx=20, pady=16)
        self._populate_master_setup_ui(host, is_standalone_wizard=False, modal_win=win)

    def _open_periodic_verify_modal(self, reason="periodic"):
        win = ctk.CTkToplevel(self)
        win.title("鹤羽音形 · 安全主密码验证")
        win.geometry("540x320")
        win.configure(fg_color=C("bg"))
        win.transient(self)
        win.grab_set()
        card = self._card(win)
        card.pack(fill="both", expand=True, padx=20, pady=18)
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="both", expand=True, padx=24, pady=20)
        if reason == "new_device":
            title_t = "🔒 检测到更换电脑设备 · 请重新登录验证主密码"
            desc_t = (
                "系统检测到当前电脑硬件设备指纹与上次授权设备不一致。\n"
                "为防止词库文件被复制到其他电脑后遭窃取，请输入您的安全主密码以授权当前设备并解密词库："
            )
            win.protocol("WM_DELETE_WINDOW", lambda: None)
        else:
            title_t = "🛡️ 每两周定期安全验证 · 请输入主密码以防遗忘"
            desc_t = (
                "距离您上次验证安全主密码已满两周（或您主动发起验证）。\n"
                "请输入一遍您的安全主密码以加深记忆，验证通过后自动重置 14 天提醒周期："
            )
        self._label(inner, title_t, 16, "bold", color="accent").pack(anchor="w")
        self._label(inner, desc_t, 13, color="text_2", wraplength=460).pack(anchor="w", pady=(8, 16))
        pw_e = self._entry(inner, 320, "请输入安全主密码…")
        pw_e.configure(show="•")
        pw_e.pack(anchor="w")
        msg_l = self._label(inner, "", 12, "bold", color="danger")
        msg_l.pack(anchor="w", pady=(8, 8))

        def _do_verify(_e=None):
            ok, vkey = _master_verify_password(DATA_DIR, pw_e.get(), self.cfg)
            if not ok or not vkey:
                msg_l.configure(text="❌ 主密码不正确，请核对后重试。")
                return
            self._unlocked_vault_key = vkey
            cur_dev = _compute_device_id(DATA_DIR)
            k_dev = _compute_device_wrap_key(DATA_DIR)
            self.cfg["authorized_device_id"] = cur_dev
            self.cfg["device_wrapped_vault_key"] = _vault_encrypt(k_dev, vkey.hex())
            self.cfg["last_password_verified_ts"] = int(time.time())
            self._cfg_touched.update(("authorized_device_id", "device_wrapped_vault_key", "last_password_verified_ts"))
            self._save_config()
            self._refresh_security_page_status()
            self._refresh_dict_lock_view()
            win.destroy()
            messagebox.showinfo("验证通过", "✓ 主密码验证通过！当前电脑设备已授权，14 天定期验证周期已更新。", parent=self)

        pw_e.bind("<Return>", _do_verify)
        self._btn(inner, "✅ 验证主密码", _do_verify, kind="primary", height=36).pack(anchor="e", pady=(6, 0))
        pw_e.focus_set()

    def _check_security_gates_on_launch(self):
        """Enforce first-run master password setup, new-device re-login, 14-day verify & 30-day backup reminders."""
        try:
            if not self._has_master_password():
                self._open_master_setup_modal(mandatory=True)
                return
            st, _ = _resolve_authorized_lexicon_key_py(DATA_DIR, self.cfg)
            if st == "new_device":
                self._open_periodic_verify_modal(reason="new_device")
                return
            now = int(time.time())
            last_v = int(self.cfg.get("last_password_verified_ts", 0))
            if last_v <= 0:
                self.cfg["last_password_verified_ts"] = now
                self._cfg_touched.add("last_password_verified_ts")
                self._save_config()
            elif (now - last_v) >= 14 * 86400:
                self._open_periodic_verify_modal(reason="periodic_14d")
                return

            last_b = int(self.cfg.get("last_lexicon_backup_ts", 0))
            if last_b <= 0:
                self.cfg["last_lexicon_backup_ts"] = now
                self._cfg_touched.add("last_lexicon_backup_ts")
                self._save_config()
            elif (now - last_b) >= 30 * 86400:
                if messagebox.askyesno(
                    "每月词库备份提醒",
                    "距离您上次导出备份词库已满一个月（30 天）。\n"
                    "为防止重要词条丢失，是否现在导出并保存最新的加密词库备份？",
                    parent=self,
                ):
                    self._export_encrypted_lexicon_backup()
                else:
                    self.cfg["last_lexicon_backup_ts"] = now
                    self._cfg_touched.add("last_lexicon_backup_ts")
                    self._save_config()
        except Exception as e:
            print(f"[studio] security gate check error: {e}", file=sys.stderr)

    # =========================================================================
    # PAGE ⑤ 快捷键 SHORTCUTS
    # =========================================================================
    def _build_page_shortcuts(self):
        page, col = self._page("快捷键", "常用快捷键与输入宏速查（只读）")
        first = True
        for gname, items in SHORTCUTS:
            g = self._group(col, gname, pady_top=16 if first else 24)
            first = False
            for title, desc, combos in items:
                r = self._row(g, title, desc)
                for i, combo in enumerate(combos):
                    if i:
                        self._label(r.slot, "/", 12, color="muted").pack(side="left", padx=6)
                    self._chip(r.slot, combo, color="text", size=13).pack(side="left")
        return page

    # =========================================================================
    # PAGE ⑥ 练习 PRACTICE
    # =========================================================================
    def _build_page_practice(self):
        page, col = self._page("练习", "字根图谱、编码反查与跟打训练", scroll=False, max_w=920)
        bar = ctk.CTkFrame(col, fg_color="transparent")
        bar.pack(fill="x", pady=(8, 0))
        self._practice_views = {}
        self._practice_cur = None
        self._practice_body = ctk.CTkFrame(col, fg_color="transparent")
        self._practice_body.pack(fill="both", expand=True, pady=(16, 0))
        opts = [("字根图谱", "atlas"), ("编码反查", "lookup"), ("跟打练习", "dojo")]
        self.practice_seg = self._choice(bar, opts, "atlas", self.practice_show)
        self.practice_seg.pack(side="left")
        self._btn(bar, "📕 打开《两周通关图文手册 (PDF)》", self.open_manual_md, kind="primary", height=32).pack(side="right")
        self.practice_show("atlas")
        return page

    def _on_show_practice(self):
        self._ensure_lexicon()

    def practice_show(self, key):
        if key not in self._practice_views:
            self._practice_views[key] = getattr(self, f"_build_practice_{key}")(self._practice_body)
        if self._practice_cur:
            self._practice_views[self._practice_cur].pack_forget()
        self._practice_cur = key
        self._practice_views[key].pack(fill="both", expand=True)
        label = {"atlas": "字根图谱", "lookup": "编码反查", "dojo": "跟打练习"}[key]
        if self.practice_seg.get() != label:
            self.practice_seg.set(label)
        if key == "lookup":
            self.after(30, lambda: self.lookup_entry.focus_set())
        elif key == "dojo":
            self.after(30, lambda: self.dojo_input.focus_set())

    def _build_practice_atlas(self, parent):
        v = ctk.CTkFrame(parent, fg_color="transparent")
        card = self._card(v)
        card.pack(fill="both", expand=True)
        top = ctk.CTkFrame(card, fg_color="transparent")
        top.pack(fill="x", padx=ROW_PX, pady=(14, 10))
        self._label(top, "26 键双拼与鹤形字根", 15, "bold").pack(anchor="w")
        self._label(top, "口诀：日月目归 O，大口三框 K · 一 A 丨 L 丿 P 丶 D 乛 V 乀 N", 12, color="muted").pack(anchor="w", pady=(3, 0))
        holder = ctk.CTkFrame(card, fg_color="transparent")
        holder.pack(fill="both", expand=True, padx=(ROW_PX - 8, 8), pady=(0, 12))
        tree = self._make_tree(holder, [("key", "键", 56, False), ("ini", "声母", 70, False), ("fin", "韵母", 110, False),
                                        ("roots", "字根", 260, True), ("ex", "例字", 300, True)], height=14)
        for i, (key, ini, fin, roots, ex) in enumerate(_load_atlas()):
            tree.insert("", "end", values=(f" {key}", f" {ini}", f" {fin}", f" {roots or '—'}", f" {ex or '—'}"),
                        tags=("odd",) if i % 2 else ())
        return v

    def _build_practice_lookup(self, parent):
        v = ctk.CTkFrame(parent, fg_color="transparent")
        card = self._card(v)
        card.pack(fill="x")
        row = ctk.CTkFrame(card, fg_color="transparent")
        row.pack(fill="x", padx=ROW_PX, pady=(16, 8))
        self.lookup_entry = self._entry(row, 300, "输入汉字或词语，例如 鹤羽音形")
        self.lookup_entry.pack(side="left", fill="x", expand=True)
        self.lookup_entry.bind("<KeyRelease>", lambda e: self._run_reverse_lookup())
        self._btn(row, "查询", self._run_reverse_lookup, kind="primary", width=88).pack(side="left", padx=(12, 0))
        self._label(card, "提示：打字时先敲前两码（双拼），候选框下方会提示各字的后两码；也可以按 ` 键加双拼反查。",
                    12, color="muted", wraplength=820).pack(anchor="w", padx=ROW_PX, pady=(0, 16))

        res = self._card(v)
        res.pack(fill="both", expand=True, pady=(16, 0))
        self.lookup_phrase = self._label(res, "", 14, "bold")
        self.lookup_phrase.pack(anchor="w", padx=ROW_PX, pady=(14, 6))
        holder = ctk.CTkFrame(res, fg_color="transparent")
        holder.pack(fill="both", expand=True, padx=(ROW_PX - 8, 8), pady=(0, 12))
        self.lookup_tree = self._make_tree(holder, [("ch", "字", 60, False), ("full", "全码", 90, False),
                                                    ("split", "拆分", 220, False), ("all", "全部可用编码", 320, True)])
        self.lookup_phrase.configure(text="输入后即时显示结果")
        return v

    def _run_reverse_lookup(self):
        query = self.lookup_entry.get().strip()
        if not query:
            return
        if self._lexicon_state != "ready":
            self.lookup_phrase.configure(text="正在加载码表…", text_color=C("muted"))
            self._ensure_lexicon(self._run_reverse_lookup)
            return
        tree = self.lookup_tree
        tree.delete(*tree.get_children())
        if query in self.short_codes:
            sc_str = ", ".join(f"{c}（{layer}）" for c, layer, _ in self.short_codes[query])
            self.lookup_phrase.configure(text=f"「{query}」 整词编码：{sc_str}", text_color=C("text"))
        else:
            self.lookup_phrase.configure(text=f"「{query}」", text_color=C("text"))
        n = 0
        for ch in dict.fromkeys(query):
            if not ("\u4e00" <= ch <= "\u9fff"):
                continue
            full_codes = self.reverse_dict.get(ch, [])
            short_c = [c for c, _, _ in self.short_codes.get(ch, [])]
            merged = list(dict.fromkeys(short_c + full_codes))
            if not merged:
                tree.insert("", "end", values=(f" {ch}", " —", " 未找到", " —"), tags=("odd",) if n % 2 else ())
            else:
                c4 = next((c for c in merged if len(c) == 4), merged[0])
                split = f" 双拼 {c4[:2]} + 鹤形 {c4[2:]}" if len(c4) == 4 else " —"
                tree.insert("", "end", values=(f" {ch}", f" {c4}", split, " " + " / ".join(merged)),
                            tags=("odd",) if n % 2 else ())
            n += 1
            if n >= 40:
                break
        if n == 0 and query not in self.short_codes:
            self.lookup_phrase.configure(text=f"未找到「{query}」的编码记录", text_color=C("danger"))

    def _build_practice_dojo(self, parent):
        v = ctk.CTkFrame(parent, fg_color="transparent")
        top = self._card(v)
        top.pack(fill="x")
        r1 = ctk.CTkFrame(top, fg_color="transparent")
        r1.pack(fill="x", padx=ROW_PX, pady=(14, 10))
        self.drill_var = ctk.StringVar(value=list(TRAINING_DRILLS.keys())[2])
        self.drill_menu = ctk.CTkOptionMenu(
            r1, variable=self.drill_var, values=list(TRAINING_DRILLS.keys()),
            width=max(320, max(self._text_w(k, self.F(13)) for k in TRAINING_DRILLS) + 56),
            height=BTN_H, corner_radius=RADIUS_CTRL, font=self.F(13), dropdown_font=self.F(13),
            fg_color=C("seg_bg"), button_color=C("seg_bg"), button_hover_color=C("nav_hover"),
            text_color=C("text"), dropdown_fg_color=C("surface"), dropdown_hover_color=C("surface_alt"),
            dropdown_text_color=C("text"), command=lambda _: self._reset_dojo(),
        )
        self.drill_menu.pack(side="left")
        self._btn(r1, "重新开始", self._reset_dojo).pack(side="right")

        stats = ctk.CTkFrame(top, fg_color="transparent")
        stats.pack(fill="x", padx=ROW_PX, pady=(0, 14))
        stats.grid_columnconfigure((0, 1), weight=0)
        stats.grid_columnconfigure(2, weight=1)
        self._dojo_stat = {}
        for i, (k, t) in enumerate([("cpm", "速度"), ("acc", "准确率"), ("hint", "下一字编码")]):
            cell = ctk.CTkFrame(stats, fg_color="transparent")
            cell.grid(row=0, column=i, sticky="w", padx=(0, 48))
            self._label(cell, t, 12, color="muted").pack(anchor="w")
            lbl = self._label(cell, "—", 17, "bold", color="accent" if k != "hint" else "text", mono=(k != "hint"))
            lbl.pack(anchor="w", pady=(2, 0))
            self._dojo_stat[k] = lbl

        tgt = self._card(v, fg="surface_alt")
        tgt.pack(fill="x", pady=(16, 0))
        self._label(tgt, "跟打文本", 12, "bold", color="muted").pack(anchor="w", padx=ROW_PX, pady=(14, 4))
        self.dojo_target_lbl = self._label(tgt, "", 20, "bold", wraplength=820)
        self.dojo_target_lbl.pack(anchor="w", padx=ROW_PX, pady=(2, 16))

        self.dojo_input = ctk.CTkTextbox(
            v, height=120, font=self.F(18), fg_color=C("surface"), text_color=C("text"), border_width=2,
            border_color=C("accent"), corner_radius=RADIUS_CARD, scrollbar_button_color=C("border_strong"),
        )
        self.dojo_input.pack(fill="both", expand=True, pady=(16, 0))
        self.dojo_input.bind("<KeyRelease>", self._on_dojo_type)
        self.dojo_start_time = None
        self.after(0, self._reset_dojo)
        return v

    def _reset_dojo(self):
        target = TRAINING_DRILLS[self.drill_var.get()]
        self.dojo_target_lbl.configure(text=target)
        self.dojo_input.delete("1.0", "end")
        self.dojo_start_time = None
        self._update_dojo_hint("")

    def _on_dojo_type(self, _event=None):
        typed = self.dojo_input.get("1.0", "end-1c")
        if not typed:
            self.dojo_start_time = None
            self._update_dojo_hint("")
            return
        if self.dojo_start_time is None:
            self.dojo_start_time = time.time()
        self._update_dojo_hint(typed)

    def _update_dojo_hint(self, typed: str):
        target = TRAINING_DRILLS[self.drill_var.get()]
        t_clean = target.replace(" ", "")
        u_clean = typed.replace(" ", "")
        elapsed = max(0.5, (time.time() - self.dojo_start_time) if self.dojo_start_time else 0.5)
        cpm = int(len(u_clean) / elapsed * 60) if u_clean else 0
        correct = sum(1 for a, b in zip(u_clean, t_clean) if a == b)
        acc = int(correct / len(u_clean) * 100) if u_clean else 100
        next_idx = len(u_clean)
        hint_str = "已完成本段"
        if next_idx < len(t_clean):
            next_ch = t_clean[next_idx]
            if self._lexicon_state != "ready":
                hint_str = f"「{next_ch}」 码表加载中…"
                self._ensure_lexicon(lambda: self._update_dojo_hint(self.dojo_input.get("1.0", "end-1c")))
            else:
                codes = self.reverse_dict.get(next_ch, [])
                shorts = [c for c, _, _ in self.short_codes.get(next_ch, [])]
                all_c = list(dict.fromkeys(shorts + codes))
                hint_str = f"「{next_ch}」 {' / '.join(all_c[:3]) if all_c else next_ch}"
        self._dojo_stat["cpm"].configure(text=f"{cpm} 字/分")
        self._dojo_stat["acc"].configure(text=f"{acc}%")
        self._dojo_stat["hint"].configure(text=hint_str)

    # =========================================================================
    # PAGE ⑦ 关于与安全 ABOUT & PRIVACY
    # =========================================================================
    def _app_version(self):
        for p in (os.path.join(BASE_DIR, "Cargo.toml"),):
            try:
                in_pkg = False
                with open(p, "r", encoding="utf-8") as f:
                    for line in f:
                        s = line.strip()
                        if s.startswith("["):
                            in_pkg = s == "[package]"
                        elif in_pkg and s.startswith("version"):
                            return s.split("=", 1)[1].strip().strip('"')
            except Exception:
                pass
        return "—"

    def _build_page_about(self):
        page, col = self._page("关于与安全", "版本信息、引擎状态、安全审计与开源许可")

        hero = self._card(col)
        hero.pack(fill="x", pady=(16, 0))
        hi = ctk.CTkFrame(hero, fg_color="transparent")
        hi.pack(fill="x", padx=ROW_PX, pady=18)
        ctk.CTkLabel(hi, text="鹤", width=52, height=52, corner_radius=14, fg_color=C("accent"),
                     text_color=C("on_accent"), font=self.F(26, "bold")).pack(side="left")
        ht = ctk.CTkFrame(hi, fg_color="transparent")
        ht.pack(side="left", padx=(16, 0))
        self._label(ht, "鹤羽音形 HEYU YinXing", 18, "bold").pack(anchor="w")
        self._label(ht, f"版本 {self._app_version()} · 纯 Rust 净室实现 · 中 / 意 / 西 / 英 四语输入", 12, color="muted").pack(anchor="w", pady=(4, 0))
        self._btn(hi, "📕 打开《两周通关图文手册 (PDF)》", self.open_manual_md, kind="primary").pack(side="right")

        g = self._group(col, "引擎")
        r = self._row(g, "引擎状态", EXE_PATH, wrap=480)
        self.about_status = self._label(r.slot, "", 13, "bold")
        self.about_status.pack(side="left", padx=(0, 16))
        self._btn(r.slot, "重启引擎", self.restart_daemon, kind="primary").pack(side="left")
        self._refresh_daemon_status()

        g = self._group(col, "安全")
        r = self._row(g, "净室二进制审计", "检查 heyu-ime.exe 的 PE 导入表：零第三方依赖、零网络库", wrap=440)
        self._btn(r.slot, "重新审计", self._refresh_security_report).pack(side="left")
        self.sec_summary = ctk.CTkFrame(g, fg_color="transparent")
        self.sec_summary.pack(fill="x", padx=ROW_PX, pady=(0, 4))
        self._sec_lines = {}
        for k, t in [("status", "安全评级"), ("deps", "第三方依赖"), ("net", "网络能力"), ("sha", "SHA-256"), ("time", "审计时间")]:
            line = ctk.CTkFrame(self.sec_summary, fg_color="transparent")
            line.pack(fill="x", pady=2)
            self._label(line, t, 12, color="muted", width=96).pack(side="left")
            v = self._label(line, "审计中…", 12, mono=(k in ("sha", "time")))
            v.pack(side="left")
            self._sec_lines[k] = v
        dt = ctk.CTkFrame(g, fg_color="transparent")
        dt.pack(fill="x", padx=ROW_PX - 8, pady=(2, 12))
        self._sec_open = False
        self.sec_toggle = ctk.CTkButton(dt, text="▸  查看完整报告", command=self._toggle_sec_details, height=28, width=10,
                                        fg_color="transparent", hover_color=C("surface_alt"), text_color=C("accent"),
                                        font=self.F(13), anchor="w")
        self.sec_toggle.pack(side="left")
        self.sec_box = self._textbox(g, mono=True, size=12, height=260)
        g._rows += 1
        r = self._row(g, "防火墙出站锁", "额外在 Windows 防火墙中禁止 heyu-ime.exe 的一切出站连接（需要管理员授权）")
        self._btn(r.slot, "添加防火墙规则", self._apply_firewall_lock, kind="danger").pack()

        g = self._group(col, "开源许可")
        for name, desc, lic in LICENSES:
            r = self._row(g, name, desc)
            self._chip(r.slot, lic, color="text_2", mono=False).pack()

        g = self._group(col, "数据来源")
        box = ctk.CTkFrame(g, fg_color="transparent")
        box.pack(fill="x", padx=ROW_PX, pady=ROW_PY)
        for line in (
            "核心码表（heyu_sys.tsv / heyu_reverse.tsv）由 tools/ 下的本地脚本编译生成，参考开源社区双拼与形码码表"
            "（rime-flypy、openfly），经净室流程清洗：移除外部链接、可执行宏和第三方品牌条目。",
            "意 / 西 / 英伴学词表为人工整理；个人词库 user_custom.tsv 仅保存在本机数据目录。",
            "输入法引擎不包含任何网络模块，所有数据只在本机读取，不上传、不联网。",
        ):
            self._label(box, line, 12, color="text_2", wraplength=740).pack(anchor="w", pady=3)
        return page

    def _on_show_about(self):
        if self._sec_report is None:
            self._sec_report = "loading"
            self._refresh_security_report()

    def _toggle_sec_details(self):
        self._sec_open = not self._sec_open
        if self._sec_open:
            self.sec_box.pack(fill="x", padx=ROW_PX, pady=(0, 16), after=self.sec_toggle.master)
            self.sec_toggle.configure(text="▾  收起完整报告")
        else:
            self.sec_box.pack_forget()
            self.sec_toggle.configure(text="▸  查看完整报告")

    def _refresh_security_report(self):
        for v in self._sec_lines.values():
            v.configure(text="审计中…", text_color=C("muted"))

        def work():
            sys.path.insert(0, os.path.join(BASE_DIR, "tools"))
            import security_audit
            return security_audit.run_security_audit()

        self._run_bg(work, self._show_security_report)

    def _show_security_report(self, rep):
        self.sec_box.configure(state="normal")
        self.sec_box.delete("1.0", "end")
        if not isinstance(rep, dict):
            self._sec_lines["status"].configure(text="审计执行失败（tools/security_audit.py 不可用）", text_color=C("danger"))
            for k in ("deps", "net", "sha", "time"):
                self._sec_lines[k].configure(text="—", text_color=C("muted"))
            self._sec_report = None
            return
        self._sec_report = rep
        deps_ok = bool(rep.get("zero_third_party_rust_crates"))
        net_ok = bool(rep.get("zero_socket_capability"))
        self._sec_lines["status"].configure(text=f"✓ {rep.get('status')}", text_color=C("success"))
        self._sec_lines["deps"].configure(text="✓ 无（Cargo.toml [dependencies] 为空）" if deps_ok else "存在依赖",
                                          text_color=C("success") if deps_ok else C("danger"))
        self._sec_lines["net"].configure(text="✓ PE 导入表中 0 个网络库" if net_ok else "检测到网络库",
                                         text_color=C("success") if net_ok else C("danger"))
        self._sec_lines["sha"].configure(text=str(rep.get("binary_sha256", "—")), text_color=C("text_2"))
        self._sec_lines["time"].configure(text=str(rep.get("timestamp", "—")), text_color=C("text_2"))
        text = (
            f"二进制路径   {rep.get('binary_path')}\n"
            f"二进制大小   {rep.get('binary_size_bytes', 0):,} Bytes\n"
            f"SHA-256      {rep.get('binary_sha256')}\n\n"
            "PE 导入表引用的 Windows 系统 DLL：\n"
        )
        for dll in rep.get("imported_os_dlls", []):
            text += f"  • {dll}\n"
        text += "\n禁止出现的网络 DLL 扫描：\n"
        for net_dll in rep.get("forbidden_network_dlls_checked", []):
            text += f"  • {net_dll:<22} 未引用 (ABSENT)\n"
        self.sec_box.insert("end", text)
        self.sec_box.configure(state="disabled")

    def _apply_firewall_lock(self):
        sys_root = os.environ.get("SystemRoot", r"C:\Windows")
        netsh_exe = os.path.join(sys_root, "System32", "netsh.exe")
        safe_exe = os.path.abspath(EXE_PATH).replace('"', "")
        args = (
            f'advfirewall firewall add rule name="HEYU_CleanRoom_Block_Out" '
            f'dir=out program="{safe_exe}" action=block enable=yes'
        )
        try:
            ctypes.windll.shell32.ShellExecuteW(0, "runas", netsh_exe, args, None, 0)
        except Exception:
            pass
        messagebox.showinfo("防火墙出站锁", "已请求写入「禁止 heyu-ime.exe 任何出站连接」的防火墙规则。\n"
                                       "加上二进制本身不含任何网络 DLL，形成双重隔离。", parent=self)

    # =========================================================================
    # DAEMON PROCESS MANAGEMENT & LIVE TELEMETRY POLLING
    # =========================================================================
    def is_daemon_running(self) -> bool:
        return _process_running("heyu-ime.exe")

    def restart_daemon(self):
        def work():
            subprocess.run(["taskkill", "/F", "/IM", "heyu-ime.exe"], creationflags=0x08000000,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            time.sleep(0.2)
            if os.path.exists(EXE_PATH):
                subprocess.Popen([EXE_PATH], cwd=os.path.dirname(EXE_PATH), creationflags=0x08000000)
            time.sleep(0.3)
            return True

        self._set_status_text(None, "正在重启…")
        self._run_bg(work, lambda _r: self._refresh_daemon_status())

    def open_manual_md(self):
        for root in (BASE_DIR, INSTALL_DIR):
            pdf_path = os.path.join(root, "docs", "鹤羽音形_两周通关图文手册.pdf")
            if os.path.exists(pdf_path):
                os.startfile(pdf_path)
                return
        for name in ("鹤羽音形_完全指南.md", "鹤羽输入法_鹤羽音形完全指南.md", "输入法_完全指南.md"):
            md_path = os.path.join(BASE_DIR, name)
            if os.path.exists(md_path):
                os.startfile(md_path)
                return
        guides = [f for f in os.listdir(BASE_DIR) if f.endswith(".md") and "指南" in f]
        if guides:
            os.startfile(os.path.join(BASE_DIR, guides[0]))
        else:
            messagebox.showinfo("使用指南", "未找到使用指南文件。", parent=self)

    def _set_status_text(self, running, text=None):
        if running is None:
            color, txt = C("muted"), text or "检测中…"
        elif running:
            color, txt = C("success"), "引擎运行中"
        else:
            color, txt = C("danger"), "引擎未运行"
        self.sb_dot.configure(fg_color=color)
        self.sb_status.configure(text=txt)
        if hasattr(self, "about_status"):
            self.about_status.configure(text=("● " + txt), text_color=color)

    def _refresh_daemon_status(self):
        running = self.is_daemon_running()
        if running != getattr(self, "_last_running", None) or hasattr(self, "about_status"):
            self._last_running = running
            self._set_status_text(running)

    def _periodic_tick(self):
        try:
            self._refresh_daemon_status()
            if self.current_page == "general":
                self._refresh_telemetry()
            elif self.current_page == "voice":
                self._refresh_model_status()
        finally:
            self.after(800, self._periodic_tick)

    def _poll_requests(self):
        """Single-instance hand-off: a second launch writes studio_request.json."""
        try:
            if os.path.exists(REQUEST_PATH):
                req = _read_json(REQUEST_PATH, {})
                try:
                    os.remove(REQUEST_PATH)
                except Exception:
                    pass
                page = req.get("page") if isinstance(req, dict) else None
                self.bring_to_front()
                if page in PAGES:
                    self.show_page(page)
        finally:
            self.after(400, self._poll_requests)

    def bring_to_front(self):
        try:
            if self.state() == "iconic":
                self.deiconify()
            self.lift()
            self.attributes("-topmost", True)
            self.after(250, lambda: self.attributes("-topmost", False))
            self.focus_force()
        except Exception:
            pass


def main():
    mini_mode = _parse_mini_arg(sys.argv[1:])
    prewarm = "--prewarm" in sys.argv[1:]
    page = _parse_page_arg(sys.argv[1:]) or "general"
    if page not in PAGES:
        page = "general"
    lock = _acquire_single_instance(page, mini_mode=mini_mode, prewarm=prewarm)
    if lock is None:
        return
    if mini_mode is None:
        try:
            if os.path.exists(REQUEST_PATH):
                os.remove(REQUEST_PATH)  # stale hand-off from a previous session
        except Exception:
            pass
    app = HeyuStudioApp(start_page=page, mini_mode=mini_mode)
    app._instance_lock = lock
    if prewarm and mini_mode is not None:
        app._mini_hidden = True
        app.withdraw()
    app.mainloop()


if __name__ == "__main__":
    main()
