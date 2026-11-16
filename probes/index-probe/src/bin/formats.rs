// Which formats yield duration/channels/rate from a header-only probe?
//
// The indexer wants to avoid decoding. symphonia's probe gives n_frames from
// the container header when the container stores it (WAV, AIFF, FLAC, MP4) and
// may not for streams that don't. This reports, per file, what a probe-only
// pass can learn, how long it takes, and whether the file can be seeked
// without decoding it first.
use std::fs::File;
use std::path::Path;
use std::time::Instant;

use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::meta::MetadataOptions;

struct Info {
    ext: String,
    codec: String,
    sr: Option<u32>,
    ch: Option<u32>,
    frames: Option<u64>,
    dur: Option<f64>,
    /// Seek to 1s from the start worked, i.e. playback can jump without a full
    /// decode. Matters for "seek past what we decoded ahead".
    seekable: bool,
    ms: f64,
}

fn probe(path: &Path) -> Option<Info> {
    let t = Instant::now();
    let file = File::open(path).ok()?;
    let mss = symphonia::core::io::MediaSourceStream::new(Box::new(file), Default::default());
    let mut probed = symphonia::default::get_probe()
        .format(
            &Default::default(),
            mss,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .ok()?;

    let track = probed.format.default_track()?.clone();
    let sr = track.codec_params.sample_rate;
    let ch = track.codec_params.channels.map(|c| c.count() as u32);
    let frames = track.codec_params.n_frames;
    let dur = match (frames, sr) {
        (Some(n), Some(s)) => Some(n as f64 / s as f64),
        _ => None,
    };
    let track_id = track.id;

    // Seek 1s in. Time-based seeking needs n_frames, so skip it when the header
    // did not provide a frame count.
    let seekable = if frames.is_some() {
        probed
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: symphonia::core::units::Time::new(1, 0.0),
                    track_id: Some(track_id),
                },
            )
            .is_ok()
    } else {
        false
    };

    let ms = t.elapsed().as_secs_f64() * 1000.0;
    Some(Info {
        ext: path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_lowercase(),
        codec: format!("{:?}", track.codec_params.codec),
        sr,
        ch,
        frames,
        dur,
        seekable,
        ms,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: formats <audio file> [more files...]");
        return;
    }
    println!(
        "{:<30} {:>5} {:>18} {:>4} {:>7} {:>9} {:>9} {:>6} {:>8}",
        "file", "ext", "codec", "ch", "rate", "frames", "duration", "seek", "probe"
    );
    for a in &args {
        let p = Path::new(a);
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("?");
        match probe(p) {
            Some(i) => println!(
                "{:<30} {:>5} {:>18} {:>4} {:>7} {:>9} {:>9} {:>6} {:>7.2}ms",
                name,
                i.ext,
                i.codec,
                i.ch.unwrap_or(0),
                i.sr.unwrap_or(0),
                i.frames
                    .map(|f| f.to_string())
                    .unwrap_or_else(|| "-".into()),
                i.dur
                    .map(|d| format!("{d:.3}s"))
                    .unwrap_or_else(|| "-".into()),
                if i.seekable { "yes" } else { "no" },
                i.ms
            ),
            None => println!("{:<30} PROBE FAILED", name),
        }
    }
}
