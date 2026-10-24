#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
鹤羽音形 (HEYU) — 萌系动画导览一键安装程序 & 完整卸载程序
=========================================================
- 运行 `python heyu_installer.py`          -> 打开带可爱仙鹤动画导览的一键安装向导
- 运行 `python heyu_installer.py --uninstall` -> 打开图形化一键完整卸载程序
"""

import os
import sys
import math
import time
import json
import shutil
import ctypes
import subprocess
import tkinter as tk
from tkinter import ttk, messagebox

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
INSTALL_DIR = r"C:\Program Files\HEYU"
PDF_MANUAL_REL = os.path.join("docs", "鹤羽音形_两周通关图文手册.pdf")


def enable_dpi_awareness():
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)
    except Exception:
        try:
            ctypes.windll.user32.SetProcessDPIAware()
        except Exception:
            pass


def find_pdf_manual() -> str:
    for root in (BASE_DIR, INSTALL_DIR):
        p = os.path.join(root, PDF_MANUAL_REL)
        if os.path.isfile(p):
            return p
    return os.path.join(BASE_DIR, PDF_MANUAL_REL)


def open_pdf_manual(parent=None):
    p = find_pdf_manual()
    if os.path.isfile(p):
        try:
            os.startfile(p)
            return True
        except Exception as e:
            if parent:
                messagebox.showwarning("打开说明书", f"无法自动打开 PDF 手册：\n{p}\n\n原因：{e}", parent=parent)
            return False
    else:
        if parent:
            messagebox.showinfo("说明书路径", f"未找到 PDF 说明书文件：\n{p}", parent=parent)
        return False


def secure_zero_wipe_file(path: str):
    try:
        if os.path.isfile(path):
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


class CuteInstallerApp(tk.Tk):
    """Cute animated one-click installer & onboarding guide for HEYU."""

    SCENES = [
        {
            "tab": "① 怎么打字最快？",
            "title": "三合一自由切换：零门槛全拼 → 2小时双拼 → 极速音形",
            "desc": (
                "• 刚装好不知道怎么用？直接选【智能全拼】，跟普通拼音一样整句连打，零学习成本！\n"
                "• 想让打字轻松一倍？花 2 小时试试【鹤羽双拼】，任何汉字只要敲 2 下（声母+韵母）。\n"
                "• 追求极致盲打？进阶【鹤羽音形】，四码唯一自动上屏，彻底告别翻页找字！"
            ),
            "badge": "全拼 / 双拼 / 音形 随时在胶囊上一键切换",
        },
        {
            "tab": "② 说话就能打字？",
            "title": "100% 离线语音转写 & 独家「双光标」黑科技",
            "desc": (
                "• 按 Ctrl+Alt+V（或点胶囊上的 🎙）对着麦克风说话，断网也能精准转成中/英/意/西文字。\n"
                "• 还能直接内录电脑里的【网课/跨国会议/视频声音】实时转成中文字幕笔记！\n"
                "• 开启【双光标独立转写】锁定笔记窗口后，你可以去别的窗口聊天改稿，语音依然稳稳写进笔记里。"
            ),
            "badge": "本地 Whisper 大模型 · 零云端上传 · 绝不偷听",
        },
        {
            "tab": "③ 胶囊与个人词库",
            "title": "灵动悬浮小胶囊 & 专属加密词库（Ctrl+Alt+N 秒造词）",
            "desc": (
                "• 桌面悬浮胶囊的大小、文字字号、显示哪些功能按钮，都能像搭积木一样自由勾选！\n"
                "• 遇到常打的专业词、地址或签名，复制后按 Ctrl+Alt+N 即可一秒加入个人置顶词库。\n"
                "• 个人词库全程采用 PBKDF2 + 256-bit 认证密文保存，日常免密添加，查看全表需验证主密码。"
            ),
            "badge": "内置 15 款插画与经典皮肤（柴犬/橘猫/熊猫/花涧/樱吹雪）",
        },
        {
            "tab": "④ 两周通关 PDF 手册",
            "title": "内置 14 页全彩《两周通关图文手册》，手把手带你上手！",
            "desc": (
                "• 担心功能太多记不住？我们为您准备了图文并茂的 14 页 PDF 说明书！\n"
                "• 内含：双拼键位速查表、两周阶梯训练计划、每个按钮的通俗图解、快捷键一页通。\n"
                "• 强烈建议安装后先点下方【📖 打开 PDF 图文说明书】花 3 分钟浏览一遍！"
            ),
            "badge": "安装后桌面上也会为您生成说明书与控制台快捷方式",
        },
    ]

    def __init__(self, uninstall_mode: bool = False):
        super().__init__()
        self.uninstall_mode = uninstall_mode
        self.title("鹤羽音形 (HEYU) · 卸载向导" if uninstall_mode else "鹤羽音形 (HEYU) · 一键安装与动画新手导览")
        self.geometry("880x690" if not uninstall_mode else "680x500")
        self.minsize(840, 650) if not uninstall_mode else self.minsize(640, 460)
        self.configure(bg="#FFFDF9")

        ico = os.path.join(BASE_DIR, "assets", "heyu.ico")
        if os.path.isfile(ico):
            try:
                self.iconbitmap(ico)
            except Exception:
                pass

        self.scene_idx = 0
        self.anim_tick = 0
        self.auto_play = True
        self.install_mode_var = tk.StringVar(value="full")
        self.open_pdf_var = tk.BooleanVar(value=True)
        self.launch_now_var = tk.BooleanVar(value=True)
        self.setup_pwd_var = tk.BooleanVar(value=True)
        self.purge_data_var = tk.BooleanVar(value=False)

        if self.uninstall_mode:
            self._build_uninstall_ui()
        else:
            self._build_install_ui()
            self._animate()

    # =========================================================================
    # INSTALLER UI & CUTE CANVAS ANIMATION
    # =========================================================================
    def _build_install_ui(self):
        # Top Header Banner
        hdr = tk.Frame(self, bg="#FFFDF9")
        hdr.pack(fill="x", padx=28, pady=(16, 8))

        title_row = tk.Frame(hdr, bg="#FFFDF9")
        title_row.pack(fill="x")
        tk.Label(
            title_row,
            text="🦢 鹤羽音形 HEYU",
            font=("Microsoft YaHei UI", 20, "bold"),
            fg="#1E293B",
            bg="#FFFDF9",
        ).pack(side="left")
        tk.Label(
            title_row,
            text="  纯离线 · 零联网 · 全拼/双拼/音形三合一 + 离线语音工作站",
            font=("Microsoft YaHei UI", 11),
            fg="#64748B",
            bg="#FFFDF9",
        ).pack(side="left", pady=(6, 0))

        pdf_top_btn = tk.Button(
            title_row,
            text="📖 先看 PDF 图文说明书",
            font=("Microsoft YaHei UI", 10, "bold"),
            fg="#B45309",
            bg="#FEF3C7",
            activebackground="#FDE68A",
            activeforeground="#92400E",
            relief="flat",
            padx=14,
            pady=5,
            cursor="hand2",
            command=lambda: open_pdf_manual(self),
        )
        pdf_top_btn.pack(side="right")

        # Scene selector pills
        tab_bar = tk.Frame(self, bg="#FFFDF9")
        tab_bar.pack(fill="x", padx=28, pady=(2, 6))
        self.tab_btns = []
        for i, sc in enumerate(self.SCENES):
            b = tk.Button(
                tab_bar,
                text=sc["tab"],
                font=("Microsoft YaHei UI", 10, "bold"),
                relief="flat",
                padx=14,
                pady=5,
                cursor="hand2",
                command=lambda idx=i: self._select_scene(idx, user_clicked=True),
            )
            b.pack(side="left", padx=(0, 8))
            self.tab_btns.append(b)

        # Animated Stage Card
        stage_wrap = tk.Frame(self, bg="#E2E8F0", bd=0, padx=1, pady=1)
        stage_wrap.pack(fill="x", padx=28, pady=(0, 10))
        stage_inner = tk.Frame(stage_wrap, bg="#FFFFFF")
        stage_inner.pack(fill="both", expand=True)

        self.canvas = tk.Canvas(
            stage_inner,
            width=820,
            height=215,
            bg="#F8FAFC",
            highlightthickness=0,
        )
        self.canvas.pack(fill="x", padx=10, pady=(10, 4))

        info_box = tk.Frame(stage_inner, bg="#FFFFFF")
        info_box.pack(fill="x", padx=18, pady=(4, 12))
        self.scene_title_lbl = tk.Label(
            info_box,
            text="",
            font=("Microsoft YaHei UI", 13, "bold"),
            fg="#0F172A",
            bg="#FFFFFF",
            anchor="w",
        )
        self.scene_title_lbl.pack(fill="x")
        self.scene_desc_lbl = tk.Label(
            info_box,
            text="",
            font=("Microsoft YaHei UI", 10),
            fg="#334155",
            bg="#FFFFFF",
            justify="left",
            anchor="w",
        )
        self.scene_desc_lbl.pack(fill="x", pady=(4, 4))
        self.scene_badge_lbl = tk.Label(
            info_box,
            text="",
            font=("Microsoft YaHei UI", 9, "bold"),
            fg="#0D9488",
            bg="#F0FDFA",
            padx=10,
            pady=3,
            anchor="w",
        )
        self.scene_badge_lbl.pack(anchor="w")

        self._select_scene(0, user_clicked=False)

        # Installation Mode Choice Card
        opt_wrap = tk.Frame(self, bg="#E2E8F0", padx=1, pady=1)
        opt_wrap.pack(fill="x", padx=28, pady=(0, 10))
        opt_card = tk.Frame(opt_wrap, bg="#FFFFFF", padx=18, pady=12)
        opt_card.pack(fill="both")

        tk.Label(
            opt_card,
            text="🛠️ 请选择最适合您的安装方式（通俗二选一）：",
            font=("Microsoft YaHei UI", 11, "bold"),
            fg="#0F172A",
            bg="#FFFFFF",
        ).pack(anchor="w", pady=(0, 6))

        r1 = tk.Radiobutton(
            opt_card,
            text="🚀 一键完整安装到 Windows 系统（最推荐：按 Win+Space 即可切换输入法，自动创建桌面图标与开机自启）",
            variable=self.install_mode_var,
            value="full",
            font=("Microsoft YaHei UI", 10, "bold"),
            fg="#0F172A",
            bg="#FFFFFF",
            activebackground="#FFFFFF",
            selectcolor="#FEF3C7",
            cursor="hand2",
        )
        r1.pack(anchor="w")

        r2 = tk.Radiobutton(
            opt_card,
            text="🎒 免管理员绿色便携模式（不写系统目录、无需管理员权限，所有配置与加密词库保存在当前文件夹）",
            variable=self.install_mode_var,
            value="portable",
            font=("Microsoft YaHei UI", 10),
            fg="#475569",
            bg="#FFFFFF",
            activebackground="#FFFFFF",
            selectcolor="#FEF3C7",
            cursor="hand2",
        )
        r2.pack(anchor="w", pady=(2, 6))

        chk_row = tk.Frame(opt_card, bg="#FFFFFF")
        chk_row.pack(fill="x", pady=(4, 0))
        tk.Checkbutton(
            chk_row,
            text="📖 安装完成后自动打开《两周通关图文手册 (PDF)》",
            variable=self.open_pdf_var,
            font=("Microsoft YaHei UI", 10, "bold"),
            fg="#B45309",
            bg="#FFFFFF",
            activebackground="#FFFFFF",
            cursor="hand2",
        ).pack(side="left", padx=(0, 16))

        tk.Checkbutton(
            chk_row,
            text="🔐 引导设置我的词库安全主密码与胶囊名字",
            variable=self.setup_pwd_var,
            font=("Microsoft YaHei UI", 10),
            fg="#334155",
            bg="#FFFFFF",
            activebackground="#FFFFFF",
            cursor="hand2",
        ).pack(side="left")

        # Bottom Action Bar
        bot = tk.Frame(self, bg="#FFFDF9")
        bot.pack(fill="x", padx=28, pady=(2, 14))

        self.status_lbl = tk.Label(
            bot,
            text="准备就绪 · 点击右侧按钮即可一键完成安装",
            font=("Microsoft YaHei UI", 10),
            fg="#64748B",
            bg="#FFFDF9",
        )
        self.status_lbl.pack(side="left")

        self.install_btn = tk.Button(
            bot,
            text="✨ 立即一键安装并开启鹤羽音形",
            font=("Microsoft YaHei UI", 12, "bold"),
            fg="#FFFFFF",
            bg="#0D9488",
            activebackground="#0F766E",
            activeforeground="#FFFFFF",
            relief="flat",
            padx=24,
            pady=9,
            cursor="hand2",
            command=self._run_install,
        )
        self.install_btn.pack(side="right")

        uninst_link = tk.Button(
            bot,
            text="卸载旧版",
            font=("Microsoft YaHei UI", 9),
            fg="#94A3B8",
            bg="#FFFDF9",
            relief="flat",
            cursor="hand2",
            command=self._switch_to_uninstall,
        )
        uninst_link.pack(side="right", padx=(0, 12))

    def _select_scene(self, idx: int, user_clicked: bool = False):
        self.scene_idx = idx % len(self.SCENES)
        if user_clicked:
            self.auto_play = False
        sc = self.SCENES[self.scene_idx]
        self.scene_title_lbl.configure(text=sc["title"])
        self.scene_desc_lbl.configure(text=sc["desc"])
        self.scene_badge_lbl.configure(text=f"✦ {sc['badge']}")
        for i, b in enumerate(self.tab_btns):
            if i == self.scene_idx:
                b.configure(bg="#0D9488", fg="#FFFFFF")
            else:
                b.configure(bg="#F1F5F9", fg="#475569")

    def _draw_cute_crane(self, c: tk.Canvas, cx: float, cy: float, t: float, happy: bool = False):
        """Draw cute animated crane mascot '羽仔' on the canvas."""
        bob = math.sin(t * 0.12) * 5.0
        y = cy + bob
        wing_flap = math.sin(t * 0.22) * 10.0

        # Soft shadow on ground
        c.create_oval(cx - 28, cy + 46, cx + 28, cy + 56, fill="#E2E8F0", outline="")

        # Legs
        c.create_line(cx - 8, y + 24, cx - 10, cy + 48, fill="#F59E0B", width=3)
        c.create_line(cx + 8, y + 24, cx + 10, cy + 48, fill="#F59E0B", width=3)

        # Tail feathers (black tip)
        c.create_oval(cx - 38, y + 2, cx - 18, y + 22, fill="#1E293B", outline="")

        # Plump white body
        c.create_oval(cx - 32, y - 16, cx + 28, y + 30, fill="#FFFFFF", outline="#CBD5E1", width=2)

        # Flapping wing with black-feathered edge
        c.create_oval(
            cx - 18,
            y - 8 + wing_flap * 0.4,
            cx + 16,
            y + 20 + wing_flap,
            fill="#F8FAFC",
            outline="#94A3B8",
            width=2,
        )
        c.create_arc(
            cx - 18,
            y - 4 + wing_flap * 0.4,
            cx + 16,
            y + 22 + wing_flap,
            start=200,
            extent=120,
            style="arc",
            outline="#334155",
            width=3,
        )

        # Neck & Head
        c.create_oval(cx + 8, y - 44, cx + 44, y - 8, fill="#FFFFFF", outline="#CBD5E1", width=2)

        # Red crown (丹顶)
        c.create_oval(cx + 18, y - 49, cx + 35, y - 37, fill="#EF4444", outline="#DC2626")

        # Cute rosy cheek
        c.create_oval(cx + 28, y - 24, cx + 38, y - 17, fill="#FDA4AF", outline="")

        # Blinking eye
        blink = (int(t) % 55) in (0, 1, 2)
        if blink or happy:
            c.create_arc(cx + 25, y - 33, cx + 35, y - 24, start=20, extent=140, style="arc", outline="#0F172A", width=2)
        else:
            c.create_oval(cx + 27, y - 33, cx + 34, y - 25, fill="#0F172A", outline="")
            c.create_oval(cx + 29, y - 32, cx + 32, y - 29, fill="#FFFFFF", outline="")

        # Golden beak
        c.create_polygon(cx + 42, y - 29, cx + 62, y - 24, cx + 42, y - 19, fill="#F59E0B", outline="#D97706")

    def _animate(self):
        if not self.winfo_exists() or self.uninstall_mode:
            return
        self.anim_tick += 1
        t = self.anim_tick
        if self.auto_play and t % 160 == 0:
            self._select_scene((self.scene_idx + 1) % len(self.SCENES), user_clicked=False)

        c = self.canvas
        c.delete("all")
        W = max(780, c.winfo_width())
        H = 215

        # Subtle background clouds / sparkles
        for i in range(5):
            sx = ((i * 170 + t * 0.8) % (W + 80)) - 40
            sy = 26 + (i % 2) * 18
            c.create_oval(sx, sy, sx + 48, sy + 18, fill="#EEF2FF", outline="")

        # Mascot on left
        self._draw_cute_crane(c, 105, 115, t, happy=(self.scene_idx == 3))

        # Scene-specific animation on right (x = 210 .. W-25)
        if self.scene_idx == 0:
            self._draw_scene_typing(c, t)
        elif self.scene_idx == 1:
            self._draw_scene_voice(c, t)
        elif self.scene_idx == 2:
            self._draw_scene_capsule(c, t)
        else:
            self._draw_scene_pdf(c, t)

        self.after(35, self._animate)

    def _draw_scene_typing(self, c: tk.Canvas, t: float):
        phase = (t // 45) % 3
        modes = [
            ("🚀 智能全拼", ["d", "a", "d", "a", "o"], "da'dao", "1. 达到  2. 大道", "零门槛整句流式解码 · 装好直接打字！", "#2563EB"),
            ("⚡ 鹤羽双拼", ["d", "a", "d", "c"], "da'dc", "1. 达到 (击键减半!)", "2 小时学会 · 任何汉字只敲 2 键！", "#0D9488"),
            ("🦅 鹤羽音形", ["d", "a", "d", "c"], "dadc", "⚡ 达到 (第4键免空格秒上屏!)", "音形合一 · 四码唯一零重码极速盲打！", "#D97706"),
        ]
        label, keys, preedit, cand_str, tip, col = modes[phase]

        # Mode pill
        c.create_rectangle(210, 22, 345, 52, fill=col, outline="")
        c.create_text(277, 37, text=label, fill="#FFFFFF", font=("Microsoft YaHei UI", 11, "bold"))
        c.create_text(365, 37, text=tip, fill="#475569", font=("Microsoft YaHei UI", 10, "bold"), anchor="w")

        # Animated keycaps
        step = int((t % 45) / 6)
        kx = 210
        for idx, k in enumerate(keys):
            pressed = idx <= step
            ky = 74 + (4 if idx == step else 0)
            bg = col if pressed else "#FFFFFF"
            fg = "#FFFFFF" if pressed else "#1E293B"
            c.create_rectangle(kx, ky, kx + 42, ky + 42, fill=bg, outline="#94A3B8", width=2)
            c.create_text(kx + 21, ky + 21, text=k.upper(), fill=fg, font=("Consolas", 14, "bold"))
            kx += 52

        # Candidate HUD preview bar
        c.create_rectangle(210, 134, 760, 192, fill="#FFFFFF", outline=col, width=2)
        c.create_text(228, 152, text=preedit if step >= 1 else "…", fill=col, font=("Consolas", 11, "bold"), anchor="w")
        c.create_text(228, 174, text=cand_str if step >= len(keys) - 1 else "正在输入编码…", fill="#0F172A", font=("Microsoft YaHei UI", 11, "bold"), anchor="w")

    def _draw_scene_voice(self, c: tk.Canvas, t: float):
        # Sound wave bars
        c.create_rectangle(210, 26, 455, 108, fill="#0F172A", outline="#334155", width=2)
        c.create_text(226, 44, text="🎙️ 正在聆听 (Ctrl+Alt+V)", fill="#38BDF8", font=("Microsoft YaHei UI", 10, "bold"), anchor="w")
        for i in range(14):
            bh = 8 + abs(math.sin(t * 0.25 + i * 0.55)) * 34
            bx = 228 + i * 15
            c.create_rectangle(bx, 82 - bh / 2, bx + 9, 82 + bh / 2, fill="#2DD4BF", outline="")

        # Dual cursor target window vs user window
        c.create_rectangle(475, 26, 775, 108, fill="#F0FDFA", outline="#0D9488", width=2)
        c.create_text(490, 46, text="📌 语音光标已锁定：会议笔记.md", fill="#0F766E", font=("Microsoft YaHei UI", 10, "bold"), anchor="w")
        typed = "「所有功能必须实用、稳定而且准确。」"
        n_chars = min(len(typed), int((t % 60) * 0.6) + 4)
        c.create_text(490, 78, text=typed[:n_chars] + "▋", fill="#0F172A", font=("Microsoft YaHei UI", 10, "bold"), anchor="w")

        c.create_rectangle(210, 124, 775, 190, fill="#FFFFFF", outline="#CBD5E1", width=2)
        c.create_text(
            226,
            146,
            text="✨ 双光标协同：你在浏览器/微信里随便点击打字，语音转写依然自动写进锁定的笔记窗口！",
            fill="#1E293B",
            font=("Microsoft YaHei UI", 10, "bold"),
            anchor="w",
        )
        c.create_text(
            226,
            170,
            text="🔊 支持【麦克风听写】+【内录电脑网课/会议音频】+【英/意/西外语实时译为中文】",
            fill="#0D9488",
            font=("Microsoft YaHei UI", 9, "bold"),
            anchor="w",
        )

    def _draw_scene_capsule(self, c: tk.Canvas, t: float):
        themes = [
            ("🌸 春日花涧", "#FFF5F7", "#DB2777", "#831843"),
            ("🐕 柴犬暖阳", "#FFFBEB", "#D97706", "#78350F"),
            ("🐼 竹林熊猫", "#F0FDF4", "#15803D", "#14532D"),
            ("🌙 暗夜黑金", "#18181B", "#F59E0B", "#FAFAFA"),
        ]
        th_name, bg, acc, fg = themes[(t // 40) % len(themes)]

        # Animated scaling pill
        scale = 1.0 + 0.08 * math.sin(t * 0.1)
        pw = int(380 * scale)
        ph = int(48 * scale)
        px, py = 220, 48

        c.create_rectangle(px, py, px + pw, py + ph, fill=bg, outline=acc, width=2)
        c.create_oval(px + 8, py + 8, px + ph - 8, py + ph - 8, fill=acc, outline="")
        c.create_text(px + ph // 2, py + ph // 2, text="鹤", fill="#FFFFFF", font=("Microsoft YaHei UI", 12, "bold"))
        c.create_text(px + ph + 10, py + ph // 2, text=f"168 字/分 · {th_name} ·  🎙  ◉  ✏  🎨  ⚙", fill=fg, font=("Microsoft YaHei UI", 10, "bold"), anchor="w")

        # Quick add card
        c.create_rectangle(220, 118, 775, 190, fill="#FFFFFF", outline="#CBD5E1", width=2)
        c.create_text(
            236,
            142,
            text="🔐 选中文字复制 → 按 Ctrl+Alt+N → 自动生成四码（如「达到 → dadc」「帮我 → bhwo」）回车秒存！",
            fill="#0F172A",
            font=("Microsoft YaHei UI", 10, "bold"),
            anchor="w",
        )
        c.create_text(
            236,
            168,
            text="🛡️ 词库落盘采用 HEYUENC1 (256-bit) 认证密文保护，Rust 引擎与设置中心实时无损同步",
            fill="#0D9488",
            font=("Microsoft YaHei UI", 9, "bold"),
            anchor="w",
        )

    def _draw_scene_pdf(self, c: tk.Canvas, t: float):
        # Illustrated PDF book card
        bx, by = 225, 28
        c.create_rectangle(bx, by, bx + 150, by + 160, fill="#FEF3C7", outline="#D97706", width=2)
        c.create_rectangle(bx + 10, by + 12, bx + 140, by + 65, fill="#D97706", outline="")
        c.create_text(bx + 75, by + 38, text="鹤羽音形\n两周通关手册", fill="#FFFFFF", font=("Microsoft YaHei UI", 11, "bold"), justify="center")
        c.create_text(bx + 75, by + 105, text="14 页高清图文\n按键逐一拆解\n双拼/音形速查表", fill="#92400E", font=("Microsoft YaHei UI", 9, "bold"), justify="center")
        c.create_text(bx + 75, by + 145, text="★ PDF 随包附带 ★", fill="#B45309", font=("Microsoft YaHei UI", 8, "bold"))

        # Right guide text + interactive prompt
        c.create_text(
            400,
            52,
            text="📘 为什么强烈建议您看一眼 PDF 说明书？",
            fill="#0F172A",
            font=("Microsoft YaHei UI", 12, "bold"),
            anchor="w",
        )
        c.create_text(
            400,
            102,
            text=(
                "1. 第一页就把所有按钮、胶囊图标、快捷键讲得明明白白；\n"
                "2. 附带【双拼键位口诀图】与【两周从全拼过渡到音形】的科学训练表；\n"
                "3. 点击右上角或勾选下方选项，安装完成瞬间自动为您打开！"
            ),
            fill="#334155",
            font=("Microsoft YaHei UI", 10),
            anchor="w",
        )
        pulse = "#F59E0B" if (t // 15) % 2 == 0 else "#0D9488"
        c.create_rectangle(400, 148, 720, 182, fill="#F0FDFA", outline=pulse, width=2)
        c.create_text(560, 165, text="👉 勾选下方「安装完成后自动打开 PDF 手册」即可直接阅读", fill="#0F766E", font=("Microsoft YaHei UI", 9, "bold"))

    # =========================================================================
    # ONE-CLICK INSTALL EXECUTION
    # =========================================================================
    def _run_install(self):
        mode = self.install_mode_var.get()
        self.install_btn.configure(state="disabled", text="⏳ 正在安装配置中，请稍候…")
        self.status_lbl.configure(text="正在配置鹤羽音形环境…", fg="#D97706")
        self.update_idletasks()

        try:
            # 1. Optional Master Password & Identity Setup Wizard
            studio_py = os.path.join(BASE_DIR, "heyu_studio.py")
            if self.setup_pwd_var.get() and os.path.isfile(studio_py):
                cfg_path = os.path.join(BASE_DIR, "data", "config.json")
                has_pwd = False
                if os.path.isfile(cfg_path):
                    try:
                        with open(cfg_path, "r", encoding="utf-8") as f:
                            has_pwd = bool(str(json.load(f).get("master_pwd_verifier", "")).strip())
                    except Exception:
                        pass
                if not has_pwd:
                    self.status_lbl.configure(text="正在打开安全主密码与胶囊个性化初始化向导…")
                    self.update_idletasks()
                    subprocess.run([sys.executable, studio_py, "--setup-wizard"], cwd=BASE_DIR)

            if mode == "portable":
                # Portable mode: create portable.flag and launch heyu-ime.exe + heyu-voice.exe directly
                with open(os.path.join(BASE_DIR, "portable.flag"), "w", encoding="utf-8") as f:
                    f.write("HEYU Portable Mode\n")
                for exe_name in ("heyu-ime.exe", "heyu-voice.exe"):
                    ep = os.path.join(BASE_DIR, exe_name)
                    if os.path.isfile(ep):
                        subprocess.Popen([ep], cwd=BASE_DIR, creationflags=0x08000000)
                self.status_lbl.configure(text="✓ 绿色便携模式已启动！", fg="#059669")
            else:
                # Full system installation via install.ps1
                ps1 = os.path.join(BASE_DIR, "install.ps1")
                if not os.path.isfile(ps1):
                    raise FileNotFoundError(f"未找到安装脚本：{ps1}")
                self.status_lbl.configure(text="正在注册 Windows TSF 输入法组件（请在弹出的权限窗口点“是”）…")
                self.update_idletasks()
                proc = subprocess.run(
                    ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", ps1, "-SourceDir", BASE_DIR],
                    cwd=BASE_DIR,
                )
                if proc.returncode != 0:
                    raise RuntimeError(f"安装脚本返回非零退出码 ({proc.returncode})")
                # Also start heyu-voice.exe if present
                for root in (INSTALL_DIR, BASE_DIR):
                    vp = os.path.join(root, "heyu-voice.exe")
                    if os.path.isfile(vp):
                        subprocess.Popen([vp], cwd=root, creationflags=0x08000000)
                        break
                self.status_lbl.configure(text="✓ 系统级安装与注册成功！按 Win + Space 即可切换到【鹤羽音形】", fg="#059669")

            if self.open_pdf_var.get():
                open_pdf_manual(self)

            messagebox.showinfo(
                "🎉 鹤羽音形安装完成！",
                "✓ 鹤羽音形已成功安装并启动！\n\n"
                "【三步极速上手】：\n"
                "1. 按键盘 Win + 空格键（或看桌面悬浮小胶囊）即可切换输入；\n"
                "2. 点击悬浮胶囊上的「⚙」即可随时切换【全拼 / 双拼 / 音形】或换皮肤；\n"
                "3. 按 Ctrl+Alt+V 可呼出离线语音听写，按 Ctrl+Alt+N 可快速添加新词。\n\n"
                "📖 已为您打开《两周通关图文手册 (PDF)》，祝您打字行云流水！",
                parent=self,
            )
            self.destroy()
        except Exception as e:
            self.install_btn.configure(state="normal", text="✨ 重新尝试一键安装")
            self.status_lbl.configure(text=f"安装未完成：{e}", fg="#DC2626")
            messagebox.showwarning("安装提示", f"安装过程中遇到问题：\n{e}\n\n您也可以选择「免管理员绿色便携模式」直接使用。", parent=self)

    # =========================================================================
    # UNINSTALLER UI & CLEAN REMOVAL
    # =========================================================================
    def _switch_to_uninstall(self):
        for w in self.winfo_children():
            w.destroy()
        self.uninstall_mode = True
        self.title("鹤羽音形 (HEYU) · 完整卸载向导")
        self.geometry("680x480")
        self._build_uninstall_ui()

    def _build_uninstall_ui(self):
        wrap = tk.Frame(self, bg="#FFFDF9", padx=32, pady=24)
        wrap.pack(fill="both", expand=True)

        tk.Label(
            wrap,
            text="🗑️ 卸载鹤羽音形 (HEYU)",
            font=("Microsoft YaHei UI", 18, "bold"),
            fg="#0F172A",
            bg="#FFFDF9",
        ).pack(anchor="w")
        tk.Label(
            wrap,
            text="本卸载程序将干净、彻底地移除以下所有系统注册项与运行进程，绝不留任何残余：",
            font=("Microsoft YaHei UI", 10),
            fg="#475569",
            bg="#FFFDF9",
        ).pack(anchor="w", pady=(6, 12))

        card = tk.Frame(wrap, bg="#FFFFFF", highlightbackground="#E2E8F0", highlightthickness=1, padx=18, pady=14)
        card.pack(fill="x", pady=(0, 14))
        steps_txt = (
            "1. 停止正在运行的后台守护进程（heyu-ime.exe 与 heyu-voice.exe）；\n"
            "2. 注销 64 位与 32 位 Windows TSF 输入法组件（heyu_tsf.dll），恢复系统默认输入法；\n"
            "3. 移除开机自启动项（HKCU Run / 计划任务 HeyuImeLauncher）与桌面快捷方式；\n"
            "4. 删除系统安装目录（C:\\Program Files\\HEYU）。"
        )
        tk.Label(card, text=steps_txt, font=("Microsoft YaHei UI", 10), fg="#334155", bg="#FFFFFF", justify="left", anchor="w").pack(fill="x")

        data_card = tk.Frame(wrap, bg="#FEF2F2", highlightbackground="#FECACA", highlightthickness=1, padx=18, pady=12)
        data_card.pack(fill="x", pady=(0, 18))
        tk.Checkbutton(
            data_card,
            text="🔥 同时彻底销毁我的个人加密词库、配置与主密码密钥（使用 0x00 全零覆写不可恢复擦除）",
            variable=self.purge_data_var,
            font=("Microsoft YaHei UI", 10, "bold"),
            fg="#B91C1C",
            bg="#FEF2F2",
            activebackground="#FEF2F2",
            cursor="hand2",
        ).pack(anchor="w")
        tk.Label(
            data_card,
            text="（若不勾选，则会保留 %APPDATA%\\HEYU 下的个人加密词库文件，方便您以后重装时继续使用）",
            font=("Microsoft YaHei UI", 9),
            fg="#7F1D1D",
            bg="#FEF2F2",
        ).pack(anchor="w", pady=(2, 0))

        bot = tk.Frame(wrap, bg="#FFFDF9")
        bot.pack(fill="x", side="bottom")
        tk.Button(
            bot,
            text="取消退出",
            font=("Microsoft YaHei UI", 10),
            fg="#475569",
            bg="#E2E8F0",
            relief="flat",
            padx=18,
            pady=8,
            cursor="hand2",
            command=self.destroy,
        ).pack(side="left")

        self.uninst_btn = tk.Button(
            bot,
            text="🗑️ 确认一键完整卸载",
            font=("Microsoft YaHei UI", 11, "bold"),
            fg="#FFFFFF",
            bg="#DC2626",
            activebackground="#B91C1C",
            activeforeground="#FFFFFF",
            relief="flat",
            padx=24,
            pady=8,
            cursor="hand2",
            command=self._run_uninstall,
        )
        self.uninst_btn.pack(side="right")

    def _run_uninstall(self):
        purge = bool(self.purge_data_var.get())
        if purge:
            if not messagebox.askyesno(
                "确认彻底销毁个人词库",
                "您勾选了【彻底销毁个人加密词库与密钥】。\n"
                "执行后您的个人词库将被 0x00 全零覆写擦除且无法恢复，确定继续吗？",
                parent=self,
            ):
                return

        self.uninst_btn.configure(state="disabled", text="⏳ 正在卸载清理中…")
        self.update_idletasks()

        # Stop heyu-voice.exe and heyu-ime.exe first
        for proc_name in ("heyu-voice.exe", "heyu-ime.exe"):
            subprocess.run(["taskkill", "/F", "/IM", proc_name], creationflags=0x08000000,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        # Remove scheduled task if present
        subprocess.run(["schtasks", "/Delete", "/TN", "HeyuImeLauncher", "/F"], creationflags=0x08000000,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        # Securely wipe user data if requested
        if purge:
            appdata_heyu = os.path.join(os.environ.get("APPDATA", ""), "HEYU")
            localappdata_heyu = os.path.join(os.environ.get("LOCALAPPDATA", ""), "HEYU")
            for root_dir in (appdata_heyu, localappdata_heyu):
                if root_dir and os.path.isdir(root_dir):
                    for dirpath, _dirnames, filenames in os.walk(root_dir):
                        for fn in filenames:
                            secure_zero_wipe_file(os.path.join(dirpath, fn))
                    shutil.rmtree(root_dir, ignore_errors=True)
            for fn in ("user_custom.tsv", "config.json", "input_history.enc", ".vault_key"):
                p = os.path.join(BASE_DIR, "data", fn)
                if os.path.isfile(p):
                    secure_zero_wipe_file(p)

        ps1 = os.path.join(BASE_DIR, "uninstall.ps1")
        if os.path.isfile(ps1):
            args = ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", ps1, "-SourceDir", BASE_DIR]
            if purge:
                args.append("-PurgeUserData")
            subprocess.run(args, cwd=BASE_DIR)

        messagebox.showinfo(
            "卸载完成",
            "✓ 鹤羽音形 (HEYU) 已从您的系统中完整卸载！\n"
            + ("✓ 个人加密词库与密钥已执行全零安全擦除。" if purge else "✓ 您的个人加密词库已保留在 %APPDATA%\\HEYU 中。"),
            parent=self,
        )
        self.destroy()


if __name__ == "__main__":
    enable_dpi_awareness()
    is_uninst = ("--uninstall" in sys.argv) or ("-u" in sys.argv)
    app = CuteInstallerApp(uninstall_mode=is_uninst)
    app.mainloop()
