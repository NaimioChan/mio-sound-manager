// Why does a valid WAV with a 20-byte fmt chunk probe to nothing?
//
// `Alarm Alert 1.wav` is `RIFF/WAVE`, `fmt ` is 20 bytes (not 16), and ffprobe
// reports pcm_s16le 44100 Hz stereo. symphonia's probe returns Ok but with no
// duration, channels or sample rate. This prints what symphonia actually got, and
// what its own WAV reader says about the file, so the cause is visible rather than
// guessed at.
//
//   cargo run --release --bin wave_fmt20 <file.wav>

use std::fs::File;
use std::path::PathBuf;

use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

fn main() {
    let path: PathBuf = std::env::args()
        .nth(1)
        .expect("usage: wave_fmt20 <file.wav>")
        .into();

    let file = File::open(&path).expect("open");
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("wav");

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions {
            enable_gapless: true,
            ..Default::default()
        },
        &MetadataOptions::default(),
    );

    match probed {
        Err(e) => println!("probe failed: {e}"),
        Ok(mut probed) => {
            println!("probe ok");
            println!("tracks: {}", probed.format.tracks().len());
            for track in probed.format.tracks() {
                println!("  track id {} params {:?}", track.id, track.codec_params);
            }
            match probed.format.default_track() {
                None => println!("default_track: none"),
                Some(t) => {
                    println!("default track id {}", t.id);
                    println!("  sample_rate {:?}", t.codec_params.sample_rate);
                    println!("  channels    {:?}", t.codec_params.channels);
                    println!("  n_frames    {:?}", t.codec_params.n_frames);
                    println!("  codec       {:?}", t.codec_params.codec);
                }
            }

            // Can a decoder be built, and what does the first packet decode to?
            if let Some(track) = probed.format.default_track().cloned() {
                let params = track.codec_params.clone();
                match symphonia::default::get_codecs().make(&params, &DecoderOptions::default()) {
                    Err(e) => println!("decoder: {e}"),
                    Ok(mut decoder) => {
                        for i in 0..3 {
                            match probed.format.next_packet() {
                                Err(e) => {
                                    println!("packet {i}: {e}");
                                    break;
                                }
                                Ok(packet) => match decoder.decode(&packet) {
                                    Err(e) => println!("decode {i}: {e}"),
                                    Ok(audio) => {
                                        println!(
                                            "decode {i}: {} frames, {} Hz, {} ch",
                                            audio.frames(),
                                            audio.spec().rate,
                                            audio.spec().channels.count()
                                        );
                                        break;
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}
