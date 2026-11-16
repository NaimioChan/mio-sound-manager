// Cold-start budget: from "decide to play this file" to "first PCM chunk in
// hand", per format. This is the decode half of cold-to-first-sample; the
// device half was measured separately.
//
// Also reports how much audio that first chunk covers and how long the whole
// file takes, so the decode-ahead window can be sized.
use std::fs::File;
use std::path::Path;
use std::time::Instant;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

struct Row {
    name: String,
    open_ms: f64,
    first_ms: f64,
    first_frames: usize,
    first_sr: u32,
    first_ch: usize,
    to_500ms_ms: f64,
    whole_ms: f64,
    total_frames: usize,
}

fn frames_of(buf: &AudioBufferRef) -> usize {
    match buf {
        AudioBufferRef::F32(b) => b.frames(),
        AudioBufferRef::S16(b) => b.frames(),
        AudioBufferRef::S24(b) => b.frames(),
        AudioBufferRef::S32(b) => b.frames(),
        AudioBufferRef::U8(b) => b.frames(),
        AudioBufferRef::S8(b) => b.frames(),
        AudioBufferRef::U16(b) => b.frames(),
        AudioBufferRef::U24(b) => b.frames(),
        AudioBufferRef::U32(b) => b.frames(),
        AudioBufferRef::F64(b) => b.frames(),
    }
}

fn run(path: &Path, target_ms: f64) -> Option<Row> {
    let t_open = Instant::now();
    let file = File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut probed = symphonia::default::get_probe()
        .format(
            &Default::default(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let track = probed.format.default_track()?.clone();
    let track_id = track.id;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).ok()?;
    let open_ms = t_open.elapsed().as_secs_f64() * 1000.0;

    let t_first = Instant::now();
    let mut first_ms = f64::NAN;
    let mut first_frames = 0usize;
    let mut sr = track.codec_params.sample_rate.unwrap_or(0);
    let mut ch = 0usize;
    let mut frames_total = 0usize;
    let mut reached_target = f64::NAN;
    let target_frames = (sr as f64 * target_ms / 1000.0) as usize;

    loop {
        let packet = match probed.format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if first_frames == 0 {
            first_frames = frames_of(&decoded);
            sr = decoded.spec().rate;
            ch = decoded.spec().channels.count();
            first_ms = t_first.elapsed().as_secs_f64() * 1000.0;
        }
        frames_total += frames_of(&decoded);
        if reached_target.is_nan() && target_frames > 0 && frames_total >= target_frames {
            reached_target = t_first.elapsed().as_secs_f64() * 1000.0;
        }
    }
    let whole_ms = t_first.elapsed().as_secs_f64() * 1000.0;

    Some(Row {
        name: path.file_name()?.to_string_lossy().into_owned(),
        open_ms,
        first_ms,
        first_frames,
        first_sr: sr,
        first_ch: ch,
        to_500ms_ms: reached_target,
        whole_ms,
        total_frames: frames_total,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    println!(
        "{:<22} {:>8} {:>9} {:>7} {:>10} {:>11} {:>10} {:>9}",
        "file", "open", "1st pkt", "frames", "sr/ch", "to 500ms", "whole", "audio"
    );
    let mut rows = Vec::new();
    for a in &args {
        for _ in 0..3 {
            if let Some(r) = run(Path::new(a), 500.0) {
                rows.push(r);
            }
        }
    }
    // Report the best of three runs per file, to remove first-touch IO noise.
    let mut seen: Vec<String> = Vec::new();
    for r in &rows {
        if !seen.contains(&r.name) {
            seen.push(r.name.clone());
        }
    }
    for name in seen {
        let mut best: Vec<&Row> = rows.iter().filter(|r| r.name == name).collect();
        best.sort_by(|a, b| a.first_ms.partial_cmp(&b.first_ms).unwrap());
        let r = best[0];
        println!(
            "{:<22} {:>7.2}ms {:>8.2}ms {:>7} {:>4}/{:>4} {:>9.2}ms {:>8.2}ms {:>8.2}s",
            r.name,
            r.open_ms,
            r.first_ms,
            r.first_frames,
            r.first_sr / 1000,
            r.first_ch,
            r.to_500ms_ms,
            r.whole_ms,
            r.total_frames as f64 / r.first_sr as f64
        );
    }
}
