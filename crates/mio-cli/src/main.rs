//! P1 acceptance harness.
//!
//! Two things this has to prove, per `docs/architecture.md` section 8:
//!
//! 1. A named file starts playing, and the measured `publish → 出声` number is
//!    in the same league as the 17.5ms the research measured on this machine.
//! 2. Rapid switching produces no glitch — no underrun, no gap, no click.
//!
//! Usage:
//!   mio-cli play <file> [--device E2x2] [--hold 2.0]
//!   mio-cli switch <file> <file> ... [--device E2x2] [--interval 120] [--rounds 3]
//!   mio-cli devices
//!   mio-cli probe <file>            (decode only, no device)

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mio_engine::{
    Backend, DecodedAudio, Decoder, Engine, EngineConfig, LatencyReport, OutputFormat,
    PublishSource,
};

mod index_cmd;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

struct Args {
    command: String,
    files: Vec<PathBuf>,
    device: Option<String>,
    backend: Backend,
    asio_frames: Option<u32>,
    hold: f64,
    interval_ms: u64,
    rounds: u32,
}

fn parse_args() -> Result<Args> {
    let mut argv = std::env::args().skip(1);
    let command = argv.next().context(
        "usage: mio-cli <play|switch|devices|probe> [files...] [--device NAME] [--hold SECS] \
         [--interval MS] [--rounds N]",
    )?;

    let mut files = Vec::new();
    let mut device = None;
    let mut backend = Backend::Wasapi;
    let mut asio_frames = None;
    let mut hold = 2.0f64;
    let mut interval_ms = 120u64;
    let mut rounds = 3u32;

    let mut rest = argv.peekable();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--device" => device = rest.next(),
            "--backend" => {
                let value = rest.next().context("--backend needs wasapi or asio")?;
                backend = Backend::parse(&value)
                    .with_context(|| format!("unknown backend {value}; expected wasapi | asio"))?;
            }
            "--asio-frames" => {
                asio_frames = Some(
                    rest.next()
                        .context("--asio-frames needs a value")?
                        .parse()
                        .context("--asio-frames must be a frame count")?,
                )
            }
            "--hold" => hold = rest.next().context("--hold needs a value")?.parse()?,
            "--interval" => {
                interval_ms = rest.next().context("--interval needs a value")?.parse()?
            }
            "--rounds" => rounds = rest.next().context("--rounds needs a value")?.parse()?,
            other if other.starts_with("--") => bail!("unknown flag {other}"),
            other => files.push(PathBuf::from(other)),
        }
    }

    Ok(Args {
        command,
        files,
        device,
        backend,
        asio_frames,
        hold,
        interval_ms,
        rounds,
    })
}

impl Args {
    /// The engine configuration these flags describe.
    fn engine_config(&self) -> EngineConfig {
        EngineConfig {
            backend: self.backend,
            device_filter: self.device.clone(),
            buffer_duration_hns: 0,
            asio_buffer_frames: self.asio_frames,
        }
    }
}

fn run() -> Result<()> {
    // The index commands take a folder and their own flags, which the playback
    // parser would reject; route them before it sees the arguments.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Some(command) = argv.first().map(|s| s.as_str()) {
        if matches!(
            command,
            "index" | "index-stats" | "search" | "classify-diff" | "probe-bench"
        ) {
            return run_index_command(command, &argv[1..]);
        }
    }

    let args = parse_args()?;
    match args.command.as_str() {
        "devices" => list_devices(),
        "probe" => probe(&args),
        "play" => play(&args),
        "switch" => switch(&args),
        "render" => render(&args),
        "classify-check" => classify_check(),
        other => bail!(
            "unknown command {other}; expected play | switch | devices | probe | render \
                        | classify-check | index | index-stats | search | classify-diff"
        ),
    }
}

fn run_index_command(command: &str, rest: &[String]) -> Result<()> {
    let folder = rest
        .first()
        .context("expected a library folder as the first argument")?;
    let args = index_cmd::parse(command, PathBuf::from(folder), &rest[1..])?;
    match command {
        "index" => index_cmd::run_index(&args),
        "index-stats" => index_cmd::run_index_stats(&args),
        "search" => index_cmd::run_search(&args),
        "classify-diff" => index_cmd::run_classify_diff(&args),
        "probe-bench" => index_cmd::run_probe_bench(&args),
        other => bail!("unhandled index command {other}"),
    }
}

/// A Python mirror of this classifier exists (`tools/classify-mirror.py`) because
/// tuning the vocabulary needs a fast loop. It is only trustworthy while it
/// agrees with the Rust implementation, so this command is the check: it reads
/// the mirror's expected output for a sample of paths and reports disagreements.
///
///     python tools/classify-mirror.py --sample 3000 --db <library.db> > expect.json
///     mio-cli classify-check expect.json
fn classify_check() -> Result<()> {
    let path = std::env::args().nth(2).context("usage: mio-cli classify-check <expect.json>")?;
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?;
    let expected: Vec<(String, String, Option<String>)> =
        serde_json::from_str(&text).with_context(|| format!("parsing {path}"))?;

    let classifier = mio_index::Classifier::new();
    let total = expected.len();
    let mut mismatches = 0usize;
    for (path, category, subcategory) in &expected {
        let got = classifier.classify(path);
        if got.category != *category || got.subcategory != *subcategory {
            mismatches += 1;
            if mismatches <= 20 {
                println!(
                    "  {}\n    mirror {category} / {subcategory:?}\n    rust   {} / {:?}",
                    path, got.category, got.subcategory
                );
            }
        }
    }
    println!("checked {total} paths, {mismatches} mismatches");
    if mismatches == 0 {
        println!("PARITY OK: the mirror reproduces the Rust classifier");
        Ok(())
    } else {
        bail!("PARITY FAILED: {mismatches} of {total} rows disagree; the mirror has drifted")
    }
}

fn list_devices() -> Result<()> {
    // Reuse the engine's own enumeration so the names printed here are the names
    // `--device` matches against. Both backends come through the same call: the
    // picker in the settings dialog draws this exact list.
    //
    // Nothing is playing in this process, so probing every ASIO driver is safe
    // and gives the channel count and preferred buffer size.
    for device in mio_engine::output::list_devices(true) {
        let mut facts = Vec::new();
        if device.default {
            facts.push("default".to_string());
        }
        if let Some(channels) = device.channels {
            facts.push(format!("{channels} ch"));
        }
        if let Some(frames) = device.buffer_frames {
            facts.push(format!("{frames} frames"));
        }
        let suffix = if facts.is_empty() {
            String::new()
        } else {
            format!("  ({})", facts.join(", "))
        };
        println!("{:<7} {}{suffix}", device.backend.label(), device.name);
    }
    Ok(())
}

fn probe(args: &Args) -> Result<()> {
    let path = args.files.first().context("probe needs a file")?;
    // Any plausible device format will do; the point is decode cost.
    let decoder = Decoder::new(OutputFormat {
        sample_rate: 48_000,
        channels: 2,
        sample: mio_engine::decode::DeviceSample::F32,
    });

    let started = Instant::now();
    let clip = decoder.decode(path)?;
    let total = started.elapsed();

    println!("file          {}", path.display());
    println!(
        "source        {} Hz, {} ch",
        clip.source_rate, clip.source_channels
    );
    println!(
        "device format {} Hz, {} ch, {}",
        clip.format.sample_rate,
        clip.format.channels,
        clip.format.sample.label()
    );
    println!(
        "audio         {:.3}s ({} frames)",
        clip.duration_s(),
        clip.frames
    );
    println!("resampled     {}", clip.resampled);
    println!("decode        {:.2} ms", clip.decode_ms);
    println!("wall clock    {:.2} ms", total.as_secs_f64() * 1000.0);
    println!("pcm bytes     {}", clip.bytes.len());
    Ok(())
}

fn play(args: &Args) -> Result<()> {
    let path = args.files.first().context("play needs a file")?;
    let engine = Engine::start(args.engine_config())?;
    banner(&engine);

    let decoder = Decoder::new(engine.format());
    let clip = Arc::new(decoder.decode(path)?);
    println!(
        "decoded {} in {:.2} ms ({:.3}s audio, {} frames)",
        path.file_name().unwrap_or_default().to_string_lossy(),
        clip.decode_ms,
        clip.duration_s(),
        clip.frames
    );

    engine.publish(Arc::clone(&clip), PublishSource::Initial);

    // Collect reports while the clip plays, so the numbers come from real
    // callbacks rather than from a synthetic measurement.
    let deadline = Instant::now() + Duration::from_secs_f64(args.hold.max(clip.duration_s() + 0.4));
    let mut reports = Vec::new();
    while Instant::now() < deadline {
        sleep(Duration::from_millis(20));
        reports.extend(engine.drain_reports());
        if engine.finished() && reports.len() >= 1 {
            // Keep going a little so the tail actually leaves the endpoint.
            sleep(Duration::from_millis(120));
            break;
        }
    }
    reports.extend(engine.drain_reports());

    println!();
    print_reports(&reports);
    println!(
        "played to   {:.3} / {:.3}s   finished={}",
        engine.position_seconds(),
        clip.duration_s(),
        engine.finished()
    );
    println!(
        "underruns   {}   silent callbacks {}",
        engine.underruns(),
        engine.silent_callbacks()
    );
    Ok(())
}

fn switch(args: &Args) -> Result<()> {
    if args.files.len() < 2 {
        bail!("switch needs at least two files");
    }

    let engine = Engine::start(args.engine_config())?;
    banner(&engine);

    let decoder = Decoder::new(engine.format());

    // Decode everything up front: this harness is measuring the audio path, not
    // the decoder. A real switch also decodes, and that cost is reported per clip.
    let mut clips: Vec<Arc<DecodedAudio>> = Vec::new();
    for path in &args.files {
        let clip = decoder.decode(path)?;
        println!(
            "decoded {:<28} {:.2} ms  {:.2}s  {}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            clip.decode_ms,
            clip.duration_s(),
            if clip.resampled {
                format!("{}→{}Hz", clip.source_rate, clip.format.sample_rate)
            } else {
                format!("{}Hz", clip.source_rate)
            }
        );
        clips.push(Arc::new(clip));
    }

    // Play the first one, then hammer the switch.
    engine.publish(Arc::clone(&clips[0]), PublishSource::Initial);
    sleep(Duration::from_millis(args.interval_ms));

    // Round 0 opens with the initial publish, so it issues one fewer switch.
    let mut issued = Vec::new();
    let started = Instant::now();

    for round in 0..args.rounds {
        for (i, clip) in clips.iter().enumerate() {
            if round == 0 && i == 0 {
                continue;
            }
            let seq = engine.publish(Arc::clone(clip), PublishSource::Switch);
            issued.push(seq);
            sleep(Duration::from_millis(args.interval_ms));
        }
    }

    let elapsed = started.elapsed();
    sleep(Duration::from_millis(120));

    let mut reports = engine.drain_reports();
    reports.sort_by(|a, b| a.seq.cmp(&b.seq));

    println!();
    println!(
        "issued {} switches in {:.1} ms",
        issued.len(),
        elapsed.as_secs_f64() * 1000.0
    );
    // One more report than switches is normal: the initial publish is a publish
    // too, and it gets measured the same way.
    println!(
        "reports         {}  ({} initial + {} of {} switches, {} superseded)",
        reports.len(),
        1,
        reports.len().saturating_sub(1).min(issued.len()),
        issued.len(),
        issued.len().saturating_sub(reports.len().saturating_sub(1))
    );
    print_reports(&reports);

    if reports.len() < issued.len() {
        println!(
            "\nnote: {} publishes produced no report. Expected whenever two switches land\n\
             inside one engine period — the callback measures the clip it actually wrote,\n\
             and the superseded publish never reached audio.",
            issued.len() - reports.len()
        );
    }

    // The glitch check. An underrun is a callback that had clip data to deliver
    // but found the endpoint empty, which is heard as a gap or a click.
    let underruns = engine.underruns();
    println!(
        "\nunderruns   {}   ({})",
        underruns,
        if underruns == 0 {
            "no audible gaps"
        } else {
            "GLITCH — the endpoint ran dry while a clip was playing"
        }
    );

    Ok(())
}

/// Decode a file to the device format and write it as a WAV.
///
/// Exists so the PCM path can be checked against the source instead of being
/// trusted: a wrong byte order or a bad channel remap sounds like noise, and no
/// latency number would reveal it.
fn render(args: &Args) -> Result<()> {
    let path = args.files.first().context("render needs a file")?;
    let out_path = args.files.get(1).context("render needs an output path")?;

    let engine = Engine::start(args.engine_config())?;
    banner(&engine);

    let format = engine.format();
    let decoder = Decoder::new(format);
    let clip = decoder.decode(path)?;

    let wav = to_wav(&clip)?;
    std::fs::write(out_path, &wav)?;
    println!(
        "wrote {} ({} bytes) from {:.2}ms decode",
        out_path.display(),
        wav.len(),
        clip.decode_ms
    );
    Ok(())
}

/// Minimal canonical WAV writer for the engine's two output formats.
fn to_wav(clip: &DecodedAudio) -> Result<Vec<u8>> {
    let bits = clip.format.sample.bytes() as u16 * 8;
    let channels = clip.format.channels as u16;
    let rate = clip.format.sample_rate;
    let block_align = channels * (bits / 8);
    let byte_rate = rate * block_align as u32;
    let data_len = clip.bytes.len() as u32;

    let mut out = Vec::with_capacity(44 + clip.bytes.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&3u16.to_le_bytes()); // IEEE float when f32, else PCM
    if bits == 16 {
        out[20..22].copy_from_slice(&1u16.to_le_bytes());
    }
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(&clip.bytes);
    Ok(out)
}

fn banner(engine: &Engine) {
    let f = engine.format();
    println!("backend       {}", engine.backend().label());
    println!("device        {}", engine.device_name());
    println!(
        "format        {} Hz, {} ch, {}",
        f.sample_rate,
        f.channels,
        f.sample.label()
    );
    println!(
        "endpoint      {} frames ({:.2} ms), engine period ~{:.2} ms",
        engine.endpoint_frames(),
        engine.endpoint_frames() as f64 / f.sample_rate as f64 * 1000.0,
        engine.engine_period_ms()
    );
}

fn print_reports(reports: &[LatencyReport]) {
    if reports.is_empty() {
        println!("no latency reports");
        return;
    }

    let write: Vec<f64> = reports.iter().map(|r| r.to_write_ms).collect();
    let queued: Vec<f64> = reports.iter().map(|r| r.queued_ms).collect();
    let audible: Vec<f64> = reports.iter().map(|r| r.to_audible_ms).collect();

    row("publish → 回调写入", &write);
    row("  of which queued", &queued);
    row("publish → 出声", &audible);
}

fn row(label: &str, values: &[f64]) {
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p = |q: f64| v[(((v.len() - 1) as f64) * q).round() as usize];
    println!(
        "{:<20} n={:<4} min={:>6.2} p50={:>6.2} p90={:>6.2} max={:>6.2}  ms",
        label,
        v.len(),
        v[0],
        p(0.5),
        p(0.9),
        v[v.len() - 1]
    );
}
