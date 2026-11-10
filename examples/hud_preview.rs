//! Offline preview renderer for the HEYU candidate window themes.
//!
//! `cargo run --release --example hud_preview` writes PNG sheets for every theme to
//! `assets/previews/` (plus `gallery.png` and `themes.json` for the control panel).
//! It uses exactly the same drawing code as the live HUD (`heyu_core::hud`).

use heyu_core::hud::{
    render_candidate_frame, render_status_pill_image, FontSet, HudLayout, HudView, OffscreenCanvas,
    RenderedImage, ThemePalette, THEMES,
};
use heyu_core::LangMode;
use std::io::Write;
use std::path::Path;

// ----------------------------------------------------------------------------
// Minimal PNG writer (stored deflate, no dependencies)
// ----------------------------------------------------------------------------
fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        table[i as usize] = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let mut raw = Vec::with_capacity((w * 4 + 1) as usize * h as usize);
    for y in 0..h as usize {
        raw.push(0u8);
        raw.extend_from_slice(&rgba[y * w as usize * 4..(y + 1) * w as usize * 4]);
    }
    let mut z = vec![0x78u8, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, c) in chunks.iter().enumerate() {
        z.push(if i + 1 == chunks.len() { 1 } else { 0 });
        let l = c.len() as u16;
        z.extend_from_slice(&l.to_le_bytes());
        z.extend_from_slice(&(!l).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut chunk = |ty: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = ty.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    let mut f = std::fs::File::create(path).expect("create png");
    f.write_all(&out).unwrap();
}

/// Premultiplied 0xAARRGGBB -> straight RGBA bytes.
fn to_rgba(px: &[u32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(px.len() * 4);
    for &p in px {
        let a = (p >> 24) & 0xFF;
        let un = |c: u32| if a == 0 { 0 } else { ((c * 255 + a / 2) / a).min(255) as u8 };
        v.extend_from_slice(&[un((p >> 16) & 0xFF), un((p >> 8) & 0xFF), un(p & 0xFF), a as u8]);
    }
    v
}

// ----------------------------------------------------------------------------
// Sheet composition helpers
// ----------------------------------------------------------------------------
fn over(dst: u32, src: u32) -> u32 {
    let sa = (src >> 24) & 0xFF;
    if sa == 0 {
        return dst;
    }
    let inv = 255 - sa;
    let ch = |s: u32, d: u32| (s + (d * inv + 127) / 255).min(255);
    let a = ch(sa, (dst >> 24) & 0xFF);
    let r = ch((src >> 16) & 0xFF, (dst >> 16) & 0xFF);
    let g = ch((src >> 8) & 0xFF, (dst >> 8) & 0xFF);
    let b = ch(src & 0xFF, dst & 0xFF);
    (a << 24) | (r << 16) | (g << 8) | b
}

fn blit(sheet: &mut OffscreenCanvas, img: &RenderedImage, x: i32, y: i32) {
    for py in 0..img.height {
        let sy = y + py;
        if sy < 0 || sy >= sheet.height {
            continue;
        }
        for px in 0..img.width {
            let sx = x + px;
            if sx < 0 || sx >= sheet.width {
                continue;
            }
            let i = (sy * sheet.width + sx) as usize;
            sheet.pixels[i] = over(sheet.pixels[i], img.pixels[(py * img.width + px) as usize]);
        }
    }
}

fn rgb_u32(r: f32, g: f32, b: f32) -> u32 {
    0xFF00_0000 | ((r.clamp(0.0, 255.0) as u32) << 16) | ((g.clamp(0.0, 255.0) as u32) << 8) | (b.clamp(0.0, 255.0) as u32)
}

/// A soft "desktop + document" backdrop so translucency and shadows are visible.
fn paint_backdrop(sheet: &mut OffscreenCanvas, dark: bool) {
    let (w, h) = (sheet.width as f32, sheet.height as f32);
    let blobs: [(f32, f32, f32, (f32, f32, f32)); 4] = if dark {
        [
            (0.15, 0.2, 0.45, (60.0, 70.0, 130.0)),
            (0.85, 0.3, 0.5, (110.0, 50.0, 90.0)),
            (0.5, 0.95, 0.55, (30.0, 90.0, 100.0)),
            (0.95, 0.95, 0.35, (90.0, 80.0, 40.0)),
        ]
    } else {
        [
            (0.12, 0.15, 0.5, (180.0, 200.0, 255.0)),
            (0.88, 0.25, 0.5, (255.0, 200.0, 220.0)),
            (0.5, 0.95, 0.6, (200.0, 240.0, 220.0)),
            (0.95, 0.95, 0.35, (255.0, 230.0, 180.0)),
        ]
    };
    let base = if dark { (24.0, 26.0, 32.0) } else { (236.0, 237.0, 242.0) };
    for y in 0..sheet.height {
        for x in 0..sheet.width {
            let (fx, fy) = (x as f32 / w, y as f32 / h);
            let mut c = base;
            for (bx, by, r, col) in blobs.iter() {
                let d = ((fx - bx) * (fx - bx) * (w / h) + (fy - by) * (fy - by)).sqrt();
                let t = (1.0 - d / r).clamp(0.0, 1.0).powf(2.0) * 0.8;
                c = (c.0 + (col.0 - c.0) * t, c.1 + (col.1 - c.1) * t, c.2 + (col.2 - c.2) * t);
            }
            sheet.pixels[(y * sheet.width + x) as usize] = rgb_u32(c.0, c.1, c.2);
        }
    }
    // faux document text lines
    let line = if dark { (60.0, 62.0, 72.0) } else { (205.0, 207.0, 216.0) };
    let mut y = 70;
    let mut k = 0;
    while y < sheet.height - 20 {
        let len = [0.82, 0.64, 0.9, 0.48, 0.74][k % 5];
        let x1 = (40.0 + (w - 80.0) * len) as i32;
        for yy in y..(y + 7).min(sheet.height) {
            for xx in 40..x1.min(sheet.width) {
                sheet.pixels[(yy * sheet.width + xx) as usize] = rgb_u32(line.0, line.1, line.2);
            }
        }
        y += 26;
        k += 1;
    }
}

/// Simulate DWM blur-behind under a rounded card region (3x box blur ≈ gaussian).
fn blur_region(sheet: &mut OffscreenCanvas, x: i32, y: i32, w: i32, h: i32, r: f32, radius: i32) {
    let pad = radius * 3;
    let (x0, y0) = ((x - pad).max(0), (y - pad).max(0));
    let (x1, y1) = ((x + w + pad).min(sheet.width), (y + h + pad).min(sheet.height));
    let (bw, bh) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut buf: Vec<[f32; 3]> = Vec::with_capacity(bw * bh);
    for yy in y0..y1 {
        for xx in x0..x1 {
            let p = sheet.pixels[(yy * sheet.width + xx) as usize];
            buf.push([((p >> 16) & 0xFF) as f32, ((p >> 8) & 0xFF) as f32, (p & 0xFF) as f32]);
        }
    }
    let rad = radius as isize;
    for _ in 0..3 {
        // horizontal
        let src = buf.clone();
        for yy in 0..bh {
            for xx in 0..bw {
                let mut acc = [0.0f32; 3];
                let mut n = 0.0;
                for k in -rad..=rad {
                    let sx = xx as isize + k;
                    if sx >= 0 && (sx as usize) < bw {
                        let s = src[yy * bw + sx as usize];
                        acc[0] += s[0];
                        acc[1] += s[1];
                        acc[2] += s[2];
                        n += 1.0;
                    }
                }
                buf[yy * bw + xx] = [acc[0] / n, acc[1] / n, acc[2] / n];
            }
        }
        let src = buf.clone();
        for yy in 0..bh {
            for xx in 0..bw {
                let mut acc = [0.0f32; 3];
                let mut n = 0.0;
                for k in -rad..=rad {
                    let sy = yy as isize + k;
                    if sy >= 0 && (sy as usize) < bh {
                        let s = src[sy as usize * bw + xx];
                        acc[0] += s[0];
                        acc[1] += s[1];
                        acc[2] += s[2];
                        n += 1.0;
                    }
                }
                buf[yy * bw + xx] = [acc[0] / n, acc[1] / n, acc[2] / n];
            }
        }
    }
    for yy in y..(y + h) {
        for xx in x..(x + w) {
            // inside rounded rect?
            let (fx, fy) = (xx as f32 + 0.5 - x as f32, yy as f32 + 0.5 - y as f32);
            let qx = (fx - w as f32 / 2.0).abs() - (w as f32 / 2.0 - r);
            let qy = (fy - h as f32 / 2.0).abs() - (h as f32 / 2.0 - r);
            let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
            if d > 0.0 {
                continue;
            }
            let s = buf[(yy - y0) as usize * bw + (xx - x0) as usize];
            // DWM also slightly lifts saturation/brightness; keep it neutral here.
            sheet.pixels[(yy * sheet.width + xx) as usize] = rgb_u32(s[0], s[1], s[2]);
        }
    }
}

// ----------------------------------------------------------------------------
// Sample content
// ----------------------------------------------------------------------------
fn view_zh() -> HudView {
    HudView {
        mode: LangMode::ZhFlypy,
        code: "hy".into(),
        ghost_tab: Some("鹤羽输入法".into()),
        candidates: vec![
            ("鹤羽".into(), String::new()),
            ("和".into(), ";".into()),
            ("话语".into(), "'".into()),
            ("黄雨".into(), String::new()),
            ("后院".into(), String::new()),
        ],
        selected: 0,
        page: 0,
        total_pages: 3,
        cpm: 0,
        shape_hints: vec![
            ("鹤".into(), "hn".into()),
            ("候".into(), "rd".into()),
            ("话".into(), "yk".into()),
            ("恢".into(), "xo".into()),
        ],
        post_commit_ghosts: vec![],
    }
}

fn view_en() -> HudView {
    HudView {
        mode: LangMode::En,
        code: "becau".into(),
        ghost_tab: None,
        candidates: vec![
            ("because".into(), "[EN] 因为".into()),
            ("became".into(), "[EN] 成为".into()),
            ("beauty".into(), "[EN] 美丽".into()),
            ("beautiful".into(), "[EN] 美丽的".into()),
            ("bécause".into(), "[IT] 因为".into()),
        ],
        selected: 0,
        page: 0,
        total_pages: 1,
        cpm: 128,
        shape_hints: vec![],
        post_commit_ghosts: vec![],
    }
}

fn view_ghost() -> HudView {
    HudView {
        mode: LangMode::ZhFlypy,
        code: String::new(),
        ghost_tab: None,
        candidates: vec![],
        selected: 0,
        page: 0,
        total_pages: 0,
        cpm: 0,
        shape_hints: vec![],
        post_commit_ghosts: vec!["的时候".into(), "以后".into(), "的话".into()],
    }
}

fn view_long() -> HudView {
    HudView {
        mode: LangMode::ZhFlypy,
        code: "zgrm".into(),
        ghost_tab: None,
        candidates: vec![
            ("中华人民共和国全国人民代表大会常务委员会关于修改的决定".into(), String::new()),
            ("中国人民".into(), ";".into()),
            ("中国人民银行".into(), "'".into()),
            ("种瓜得瓜".into(), String::new()),
        ],
        selected: 0,
        page: 0,
        total_pages: 1,
        cpm: 0,
        shape_hints: vec![],
        post_commit_ghosts: vec![],
    }
}

fn render(view: &HudView, t: &ThemePalette, fonts: &FontSet, s: f32, layout: HudLayout, glass: bool) -> (RenderedImage, i32, i32, i32, i32, i32) {
    let fr = render_candidate_frame(view, t, fonts, s, layout, glass, (900.0 * s) as i32);
    let (cx, cy, cw, ch, r) = (fr.card_x, fr.card_y, fr.card_w, fr.card_h, fr.radius);
    (fr.canvas.into_image(), cx, cy, cw, ch, r)
}

fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn main() {
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("previews");
    std::fs::create_dir_all(&out_dir).unwrap();
    let fonts = FontSet::new();
    let scale: f32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let mut json = String::from("[\n");
    let mut gallery_imgs: Vec<(RenderedImage, bool)> = Vec::new();

    for (ti, t) in THEMES.iter().enumerate() {
        let a = render(&view_zh(), t, &fonts, scale, HudLayout::Horizontal, false);
        let b = render(&view_en(), t, &fonts, scale, HudLayout::Vertical, false);
        let c = render(&view_ghost(), t, &fonts, scale, HudLayout::Horizontal, false);
        let d = render(&view_long(), t, &fonts, scale, HudLayout::Horizontal, false);
        let p1 = render_status_pill_image(LangMode::ZhFlypy, 86, t, &fonts, scale);
        let p2 = render_status_pill_image(LangMode::En, 0, t, &fonts, scale);
        let g = if t.glass { Some(render(&view_zh(), t, &fonts, scale, HudLayout::Horizontal, true)) } else { None };

        let pad = (28.0 * scale) as i32;
        let title_h = (44.0 * scale) as i32;
        let col2_w = c.0.width.max(p1.width).max(p2.width);
        let mut sheet_w = (a.0.width.max(d.0.width)).max(b.0.width + col2_w + pad) + pad * 2;
        if let Some(gg) = &g {
            sheet_w = sheet_w.max(gg.0.width + pad * 2);
        }
        let right_col_h = c.0.height + p1.height + p2.height + pad;
        let mut sheet_h = title_h + pad + a.0.height + b.0.height.max(right_col_h) + d.0.height + pad;
        if let Some(gg) = &g {
            sheet_h += gg.0.height + (30.0 * scale) as i32;
        }
        let mut sheet = OffscreenCanvas::new(sheet_w, sheet_h);
        paint_backdrop(&mut sheet, t.is_dark);
        let title_col = if t.is_dark { (235, 235, 240) } else { (30, 30, 36) };
        let sub_col = if t.is_dark { (150, 150, 160) } else { (110, 110, 120) };
        let tw = sheet.draw_text(fonts.font_cand_main, t.name, pad, pad / 2 + 4, title_col);
        sheet.draw_text(
            fonts.font_cand_sub,
            &format!("  #{}  {}{}", ti, if t.is_dark { "dark" } else { "light" }, if t.glass { " · glass" } else { "" }),
            pad + tw,
            pad / 2 + 10,
            sub_col,
        );
        let mut y = title_h;
        blit(&mut sheet, &a.0, pad - a.1, y);
        y += a.0.height;
        blit(&mut sheet, &b.0, pad - b.1, y);
        let x2 = pad - b.1 + b.0.width + pad / 2;
        blit(&mut sheet, &c.0, x2, y);
        blit(&mut sheet, &p1, x2 + c.1, y + c.0.height + pad / 2);
        blit(&mut sheet, &p2, x2 + c.1, y + c.0.height + p1.height + pad / 2);
        y += b.0.height.max(right_col_h);
        blit(&mut sheet, &d.0, pad - d.1, y);
        y += d.0.height;
        if let Some(gg) = &g {
            sheet.draw_text(fonts.font_cand_sub, "Win11 毛玻璃 (blur-behind) 模拟", pad, y, sub_col);
            y += (24.0 * scale) as i32;
            let (gx, gy) = (pad, y);
            blur_region(&mut sheet, gx, gy, gg.3, gg.4, gg.5 as f32, (10.0 * scale) as i32);
            blit(&mut sheet, &gg.0, gx, gy);
        }

        let file = format!("theme_{:02}_{}.png", ti, t.slug);
        let path = out_dir.join(if (scale - 1.0).abs() < 0.01 { file.clone() } else { format!("scale{}_{}", (scale * 100.0) as i32, file) });
        write_png(&path, sheet.width as u32, sheet.height as u32, &to_rgba(&sheet.pixels));
        println!("wrote {}", path.display());

        json.push_str(&format!(
            "  {{\"index\": {}, \"name\": \"{}\", \"slug\": \"{}\", \"is_dark\": {}, \"glass\": {}, \"file\": \"{}\", \"accent\": \"{}\", \"bg\": \"{}\", \"text\": \"{}\"}}{}\n",
            ti, t.name, t.slug, t.is_dark, t.glass, file, hex(t.accent), hex((t.bg_top.0, t.bg_top.1, t.bg_top.2)), hex(t.text_primary),
            if ti + 1 < THEMES.len() { "," } else { "" }
        ));
        gallery_imgs.push((render(&view_zh(), t, &fonts, scale, HudLayout::Horizontal, false).0, t.is_dark));
    }
    json.push_str("]\n");
    if (scale - 1.0).abs() < 0.01 {
        std::fs::write(out_dir.join("themes.json"), json).unwrap();
        // Gallery: two columns (light | dark)
        let cell_w = gallery_imgs.iter().map(|g| g.0.width).max().unwrap_or(400);
        let cell_h = gallery_imgs.iter().map(|g| g.0.height).max().unwrap_or(150);
        let lights: Vec<&RenderedImage> = gallery_imgs.iter().filter(|g| !g.1).map(|g| &g.0).collect();
        let darks: Vec<&RenderedImage> = gallery_imgs.iter().filter(|g| g.1).map(|g| &g.0).collect();
        let rows = lights.len().max(darks.len()) as i32;
        let mut gal = OffscreenCanvas::new(cell_w * 2 + 40, rows * cell_h + 40);
        // left half light backdrop, right half dark
        let half = gal.width / 2;
        for yy in 0..gal.height {
            for xx in 0..gal.width {
                gal.pixels[(yy * gal.width + xx) as usize] = if xx < half { 0xFFE9EAEF } else { 0xFF1C1D22 };
            }
        }
        for (i, im) in lights.iter().enumerate() {
            blit(&mut gal, im, 10, 20 + i as i32 * cell_h);
        }
        for (i, im) in darks.iter().enumerate() {
            blit(&mut gal, im, half + 10, 20 + i as i32 * cell_h);
        }
        let p = out_dir.join("gallery.png");
        write_png(&p, gal.width as u32, gal.height as u32, &to_rgba(&gal.pixels));
        println!("wrote {}", p.display());
    }
}
