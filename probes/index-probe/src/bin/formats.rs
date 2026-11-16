// Which formats yield duration/channels/rate from a header-only probe?
//
// The indexer wants to avoid decoding. symphonia's probe gives n_frames from
// the container header when the container stores it (WAV, AIFF, FLAC, MP4) and
// may not for streams that don't (bare MP3, some OGG). This reports, per file,
// what a probe-only pass can learn, and how long it takes.
use std::fs::File;
use std::path::Path;
use std::time::Instant;

use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::meta::MetadataOptions;

struct Info {
    container: String,
    codec: String,
    sr: Option<u32>,
    ch: Option<u32>,
    frames: Option<u64>,
    dur: Option<f64>,
    seekable: bool,
    ms: f64,
}

fn probe(path: &Path) -> Option<Info> {
    let t = Instant::now();
    let file = File::open(path).ok()?;
    let mss = symphonia::core::io::MediaSourceStream::new(Box::new(file), Default::default());
    let probed = symphonia::default::get_probe()
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
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    Some(Info {
        container: "?".to_string(),
        codec: format!("{:?}", track.codec_params.codec),
        sr,
        ch,
        frames,
        dur,
        seekable: false,
        ms,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for a in &args {
        let p = Path::new(a);
        match probe(p) {
            Some(i) => println!(
                "{:<44} {:>6} {:?} {}ch {}Hz frames={:?} dur={:?} seek={} {:.2}ms",
                p.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                i.container,
                i.codec,
                i.ch.unwrap_or(0),
                i.sr.unwrap_or(0),
                i.frames,
                i.dur.map(|d| format!("{d:.3}s")),
                i.seekable,
                i.ms
            ),
            None => println!(
                "{:<44} PROBE FAILED",
                p.file_name().and_then(|s| s.to_str()).unwrap_or("?")
            ),
        }
    }
}
