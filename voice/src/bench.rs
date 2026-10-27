//! `heyu-voice.exe --simulate-live <wav> --pipeline old|new [--model M] [--llm MODEL] [--cpu] [--out F] [--data-dir D]`
//! Replays a WAV through the SAME chunking rules as continuous mic mode (pause-based flush),
//! the same rolling-context prompt, and either the v1 (old) or v2 (new) post-processing,
//! printing every chunk with latency. Used for before/after benchmarks; never types anything.

use crate::{asr, audio, config::Dirs, corrector, polish};
use std::path::PathBuf;
use std::time::Instant;

fn arg_val(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned())
}

/// Same thresholds as `continuous_mode` mic flush in main.rs (20 ms frames, energy VAD).
fn live_chunks(pcm: &[i16], fast: bool) -> Vec<(usize, usize)> {
    let fr = 320usize;
    let nfr = pcm.len() / fr;
    let mut res = Vec::new();
    let mut start = 0usize;
    let mut speech_started = false;
    let mut speech_ms = 0u64;
    let mut silence_ms = 0u64;
    let mut sil_start_fr = 0usize;
    for f in 0..nfr {
        let s = &pcm[f * fr..(f + 1) * fr];
        let e: f64 = s.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / fr as f64;
        let db = 10.0 * (e.max(1.0) / (32768.0f64 * 32768.0)).log10();
        if db > -42.0 {
            speech_started = true;
            speech_ms += 20;
            silence_ms = 0;
        } else {
            if silence_ms == 0 { sil_start_fr = f; }
            silence_ms += 20;
        }
        let chunk_el = ((f + 1) * fr - start) as u64 / 16;
        let pause_ready = speech_started && if fast {
            (chunk_el >= 2_800 && speech_ms >= 750 && silence_ms >= 760)
                || (chunk_el >= 5_500 && speech_ms >= 1_400 && silence_ms >= 580)
                || (speech_ms >= 350 && silence_ms >= 1_200)
        } else {
            (chunk_el >= 1_800 && silence_ms >= 560) || (chunk_el >= 3_800 && silence_ms >= 440) || silence_ms >= 780
        };
        let full = speech_started && chunk_el >= 16_000;
        if pause_ready || full {
            let cut = if pause_ready { ((sil_start_fr * fr) + (f + 1) * fr) / 2 } else { (f + 1) * fr };
            res.push((start, cut));
            start = cut;
            speech_started = false;
            speech_ms = 0;
            silence_ms = 0;
        }
    }
    if pcm.len() > start + 3200 {
        res.push((start, pcm.len()));
    }
    res
}

pub fn run(argv: &[String]) -> i32 {
    let wav = PathBuf::from(arg_val(argv, "--simulate-live").unwrap_or_default());
    let pipeline = arg_val(argv, "--pipeline").unwrap_or_else(|| "new".into());
    let new = pipeline != "old";
    let dirs = Dirs::resolve(arg_val(argv, "--data-dir").map(PathBuf::from));
    let (cfg, _) = dirs.load_config();
    let model = arg_val(argv, "--model").unwrap_or(cfg.model.clone());
    let llm = arg_val(argv, "--llm");
    let cpu = argv.iter().any(|a| a == "--cpu");
    let Some(runtime) = dirs.runtime_dir() else { println!("error=RUNTIME_MISSING"); return 2 };
    let gpu = !cpu && asr::runtime_has_cuda(&runtime);
    let pcm = match audio::read_audio_any_16k_mono(&wav) { Ok(p) => p, Err(e) => { println!("error={}", e); return 2 } };
    let hotwords = if new { polish::merged_hotwords(&dirs) } else { dirs.load_custom_hotwords() };
    let threads = asr::physical_cores();
    let t_load = Instant::now();
    let mut w = match asr::Whisper::load(&runtime, &model, gpu, false) { Ok(w) => w, Err(e) => { println!("error={}", e); return 3 } };
    let _ = w.transcribe(&vec![0.0f32; 16000], "zh", None, threads);
    println!("pipeline={} model={} gpu={} llm={} load_ms={} audio_s={:.1}", pipeline, model, gpu, llm.clone().unwrap_or("-".into()), t_load.elapsed().as_millis(), pcm.len() as f32 / 16000.0);
    let pc = polish::PolishCfg {
        enabled: llm.is_some() && new,
        model: llm.clone().unwrap_or_default(),
        timeout_ms: 2500,
        hotwords: hotwords.iter().take(24).cloned().collect::<Vec<_>>().join("、"),
    };
    if pc.enabled {
        let t = Instant::now();
        let ok = polish::llm_warmup(&pc);
        println!("llm_warmup ok={} ms={}", ok, t.elapsed().as_millis());
    }
    let mut ctx = corrector::SessionContext::new();
    if let Some(sf) = arg_val(argv, "--seed-tail") {
        let seed = std::fs::read_to_string(sf).unwrap_or_default();
        ctx.observe_committed(seed.trim());
        println!("seeded_tail_chars={}", seed.trim().chars().count());
    }
    let mut full = String::new();
    let mut lat: Vec<u128> = Vec::new();
    let chunks = live_chunks(&pcm, new);
    let nchunks = chunks.len();
    for (ci, (a, b)) in chunks.into_iter().enumerate() {
        let is_last = ci + 1 == nchunks;
        if !audio::preprocess_and_verify_human_speech(&pcm[a..b]) { println!("chunk={} skipped (no speech)", ci + 1); continue; }
        let f = asr::pcm_to_f32(&pcm[a..b]);
        let t0 = Instant::now();
        let prompt = if new {
            Some(polish::build_prompt_v2(&hotwords, ctx.recent_prompt_tail()))
        } else {
            asr::build_prompt_with_context("zh", cfg.simplified_chinese, &hotwords, ctx.recent_prompt_tail())
        };
        let raw = match w.transcribe(&f, "zh", prompt.as_deref(), threads) { Ok(t) => t, Err(e) => { println!("error={}", e); return 3 } };
        let asr_ms = t0.elapsed().as_millis();
        let odd: Vec<String> = raw.chars().filter(|c| (0xFE10..=0xFE6F).contains(&(*c as u32)) || matches!(*c, '～' | '〜')).map(|c| format!("U+{:04X}", c as u32)).collect();
        let draft = if new {
            polish::hallucination_guard(&polish::postprocess_v2(&raw, prompt.as_deref(), cfg.smart_correction), (b - a) / 16)
        } else {
            asr::postprocess_full(&raw, "zh", prompt.as_deref(), cfg.smart_correction, cfg.simplified_chinese, is_last, Some(&ctx))
        };
        let (fin, acc, pms) = if new { polish::llm_polish(&pc, ctx.recent_prompt_tail(), &draft) } else { (draft.clone(), false, 0) };
        let fin = if new { fin } else { corrector::apply_replace_rules(&fin, &dirs.load_custom_replace_rules(&cfg.replace_rules)) };
        let fin = ctx.dedup_boundary_prefix(&fin);
        ctx.observe_committed(&fin);
        full.push_str(&fin);
        let total = t0.elapsed().as_millis();
        lat.push(total);
        println!("chunk={} audio_ms={} asr_ms={} polish_ms={} accepted={} odd=[{}]\n  raw  : {}\n  draft: {}\n  final: {}", ci + 1, (b - a) / 16, asr_ms, pms, acc, odd.join(","), raw.trim(), draft, fin);
    }
    lat.sort();
    let avg = lat.iter().sum::<u128>() as f64 / lat.len().max(1) as f64;
    let p90 = lat.get(((lat.len() as f64) * 0.9) as usize).or(lat.last()).copied().unwrap_or(0);
    println!("SUMMARY chunks={} avg_latency_ms={:.0} p90_latency_ms={}", lat.len(), avg, p90);
    println!("TEXT={}", full);
    if let Some(o) = arg_val(argv, "--out") {
        let _ = std::fs::write(o, &full);
    }
    0
}
pub fn probe(argv: &[String]) -> i32 {
    let p = arg_val(argv, "--probe").unwrap_or_default();
    let raw = std::fs::read_to_string(&p).unwrap_or_default();
    let ctx = corrector::SessionContext::new();
    let mut out = String::new();
    for line in raw.lines() {
        let line = line.replace("\\n", "\n");
        for fin in [false, true] {
            let a = asr::postprocess_full(&line, "zh", Some("以下是普通话的句子。"), true, true, fin, Some(&ctx));
            let simp = corrector::to_simplified_zh(&line);
            let cps = |s: &str| s.chars().filter(|c| !(0x4E00..=0x9FFF).contains(&(*c as u32))).map(|c| format!("{:04X}", c as u32)).collect::<Vec<_>>().join(" ");
            out.push_str(&format!("IN [{}] final={}\n OUT[{}] cps={}\n SIMP cps={}\n V2 [{}]\n", line, fin, a, cps(&a), cps(&simp), polish::postprocess_v2(&line, None, true)));
        }
    }
    let _ = std::fs::write(format!("{}.out.txt", p), out);
    0
}