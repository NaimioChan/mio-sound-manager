// IAudioClient3 shared-mode low-period probe.
//
// Answers: on this machine, can WASAPI shared mode (no exclusive lock, other
// apps keep working) reach a single-digit-millisecond publish-to-first-render
// latency? Measures the engine period range the driver advertises, then times
// "publish a new clip" -> "render thread has written it to the device".
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use wasapi::*;

struct Shared {
    pub_seq: AtomicU64,
    pub_ns: AtomicU64,
    t0: Instant,
}

impl Shared {
    fn publish(&self) {
        let ns = self.t0.elapsed().as_nanos() as u64;
        self.pub_ns.store(ns, Ordering::Relaxed);
        self.pub_seq.fetch_add(1, Ordering::Release);
    }
    fn now_ns(&self) -> u64 {
        self.t0.elapsed().as_nanos() as u64
    }
}

fn list_devices() {
    initialize_mta().ok().unwrap();
    let en = DeviceEnumerator::new().unwrap();
    println!("Render devices:");
    for d in &en.get_device_collection(Direction::Render).unwrap() {
        let d = d.unwrap();
        println!(
            "  {:?}  state={:?}",
            d.get_friendlyname().unwrap(),
            d.get_state().unwrap()
        );
    }
    for role in [Role::Console, Role::Multimedia, Role::Communications] {
        if let Ok(d) = en.get_default_device_for_role(Direction::Render, role) {
            println!("  default {:?}: {:?}", role, d.get_friendlyname().unwrap());
        }
    }
}

fn pick_device(name_filter: Option<&str>) -> Device {
    let en = DeviceEnumerator::new().unwrap();
    match name_filter {
        None => en.get_default_device(Direction::Render).unwrap(),
        Some(f) => {
            for d in &en.get_device_collection(Direction::Render).unwrap() {
                let d = d.unwrap();
                let n = d.get_friendlyname().unwrap();
                if n.to_lowercase().contains(&f.to_lowercase()) {
                    return d;
                }
            }
            panic!("no render device matching {f}");
        }
    }
}

fn stats(mut v: Vec<f64>, label: &str) {
    if v.is_empty() {
        println!("{label}: no samples");
        return;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    let pct = |p: f64| v[((n as f64 - 1.0) * p).round() as usize];
    println!(
        "{label}: n={n} median={:.2}ms min={:.2} p90={:.2} max={:.2}",
        pct(0.5),
        v[0],
        pct(0.9),
        v[n - 1]
    );
}

/// One run: initialize a shared stream at `period_in_frames` and measure.
fn run_shared(
    dev: &Device,
    period_frames: Option<u32>,
    duration: Duration,
    label: &str,
) -> Option<(u32, f64, f64, f64)> {
    let mut client = dev.get_iaudioclient().unwrap();
    let mix = client.get_mixformat().unwrap();
    let sr = mix.get_samplespersec();
    let ch = mix.get_nchannels();
    let blockalign = mix.get_blockalign() as usize;

    match client.initialize_shared_audio_stream(&mix, TimingMode::Events, period_frames.unwrap_or(0)) {
        Ok(()) => {}
        Err(e) => {
            println!("  [{label}] initialize_shared_audio_stream({period_frames:?}) failed: {e}");
            return None;
        }
    }

    let buf_frames = client.get_buffer_size().unwrap();
    let ev = client.set_get_eventhandle().unwrap();
    let render = client.get_audiorenderclient().unwrap();

    let sh = Arc::new(Shared {
        pub_seq: AtomicU64::new(0),
        pub_ns: AtomicU64::new(0),
        t0: Instant::now(),
    });

    // Prime the device with silence so the first event has space to write.
    let space = client.get_available_space_in_frames().unwrap();
    if space > 0 {
        let silence = vec![0u8; space as usize * blockalign];
        render.write_to_device(&silence, None).unwrap();
    }

    client.start_stream().unwrap();

    let sh2 = sh.clone();
    let publisher = thread::spawn(move || {
        // Publish a new "clip" every 40ms, like a user clicking rows.
        for _ in 0..(duration.as_millis() / 40) {
            thread::sleep(Duration::from_millis(40));
            sh2.publish();
        }
    });

    let mut lat = Vec::new();
    let mut blocks = Vec::new();
    let mut last_seen = 0u64;
    let start = Instant::now();
    let deadline = start + duration;

    while Instant::now() < deadline {
        if ev.wait_for_event(100).is_err() {
            println!("  [{label}] event wait timed out");
            break;
        }
        let space = client.get_available_space_in_frames().unwrap();
        if space == 0 {
            continue;
        }
        blocks.push(space);

        // Detect a publish that this callback is the first to consume.
        let seq = sh.pub_seq.load(Ordering::Acquire);
        if seq != last_seen {
            let pub_ns = sh.pub_ns.load(Ordering::Relaxed);
            let now = sh.now_ns();
            lat.push((now.saturating_sub(pub_ns)) as f64 / 1e6);
            last_seen = seq;
        }

        let data = vec![0u8; space as usize * blockalign];
        render.write_to_device(&data, None).unwrap();
    }

    let _ = publisher.join();

    blocks.sort();
    let n = blocks.len();
    let med_block = if n > 0 { blocks[n / 2] } else { 0 };
    let period_ms = med_block as f64 / sr as f64 * 1000.0;
    println!(
        "  [{label}] {sr}Hz {ch}ch buf={buf_frames} frames, callbacks={n}, \
         frames/callback median={med_block} ({period_ms:.2}ms)"
    );
    stats(lat.clone(), &format!("  [{label}] publish->render"));

    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = if lat.is_empty() {
        f64::NAN
    } else {
        lat[lat.len() / 2]
    };
    let max = lat.last().copied().unwrap_or(f64::NAN);
    Some((med_block, period_ms, median, max))
}

fn run_exclusive(dev: &Device, duration: Duration) {
    let mut client = dev.get_iaudioclient().unwrap();
    let mix = client.get_mixformat().unwrap();
    let sr = mix.get_samplespersec();
    let (def_hns, min_hns) = client.get_device_period().unwrap();
    println!(
        "  [exclusive] device period default={:.2}ms min={:.2}ms",
        def_hns as f64 / 1e4,
        min_hns as f64 / 1e4
    );
    let mode = StreamMode::EventsExclusive {
        period_hns: min_hns,
    };
    if let Err(e) = client.initialize_client(&mix, Direction::Render, mode, None) {
        println!("  [exclusive] init failed: {e}");
        return;
    }
    let blockalign = mix.get_blockalign() as usize;
    let buf_frames = client.get_buffer_size().unwrap();
    let ev = client.set_get_eventhandle().unwrap();
    let render = client.get_audiorenderclient().unwrap();

    let sh = Arc::new(Shared {
        pub_seq: AtomicU64::new(0),
        pub_ns: AtomicU64::new(0),
        t0: Instant::now(),
    });
    let space = client.get_available_space_in_frames().unwrap();
    if space > 0 {
        render
            .write_to_device(&vec![0u8; space as usize * blockalign], None)
            .unwrap();
    }
    client.start_stream().unwrap();

    let sh2 = sh.clone();
    let publisher = thread::spawn(move || {
        for _ in 0..(duration.as_millis() / 40) {
            thread::sleep(Duration::from_millis(40));
            sh2.publish();
        }
    });

    let mut lat = Vec::new();
    let mut blocks = Vec::new();
    let mut last_seen = 0u64;
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if ev.wait_for_event(100).is_err() {
            break;
        }
        let space = client.get_available_space_in_frames().unwrap();
        if space == 0 {
            continue;
        }
        blocks.push(space);
        let seq = sh.pub_seq.load(Ordering::Acquire);
        if seq != last_seen {
            let pub_ns = sh.pub_ns.load(Ordering::Relaxed);
            lat.push((sh.now_ns().saturating_sub(pub_ns)) as f64 / 1e6);
            last_seen = seq;
        }
        render
            .write_to_device(&vec![0u8; space as usize * blockalign], None)
            .unwrap();
    }
    let _ = publisher.join();
    blocks.sort();
    let n = blocks.len();
    let med_block = if n > 0 { blocks[n / 2] } else { 0 };
    println!(
        "  [exclusive] {sr}Hz buf={buf_frames} frames, callbacks={n}, \
         frames/callback median={med_block} ({:.2}ms)",
        med_block as f64 / sr as f64 * 1000.0
    );
    stats(lat, "  [exclusive] publish->render");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("periods");

    initialize_mta().unwrap();

    if cmd == "devices" {
        list_devices();
        return;
    }

    let filter: Option<&str> = args.get(2).map(|s| s.as_str()).filter(|s| *s != "-");
    let dev = pick_device(filter);
    println!("device: {:?}", dev.get_friendlyname().unwrap());

    match cmd {
        "periods" => {
            let mut client = dev.get_iaudioclient().unwrap();
            let mix = client.get_mixformat().unwrap();
            println!(
                "mix format: {}Hz {}ch {}bit {:?}",
                mix.get_samplespersec(),
                mix.get_nchannels(),
                mix.get_bitspersample(),
                mix.get_subformat().unwrap()
            );
            let (def_hns, min_hns) = client.get_device_period().unwrap();
            println!(
                "device period: default={:.2}ms min={:.2}ms",
                def_hns as f64 / 1e4,
                min_hns as f64 / 1e4
            );
            match client.get_shared_mode_engine_period(&mix) {
                Ok(p) => {
                    let sr = mix.get_samplespersec() as f64;
                    println!(
                        "shared engine period: default={} fundamental={} min={} max={} frames \
                         ({:.2}ms default, {:.2}ms min)",
                        p.default,
                        p.fundamental,
                        p.min,
                        p.max,
                        p.default as f64 / sr * 1000.0,
                        p.min as f64 / sr * 1000.0
                    );
                }
                Err(e) => println!("get_shared_mode_engine_period failed: {e}"),
            }
        }
        "shared" => {
            // Baseline: the engine default period (what cpal gives today).
            println!("== shared, engine default period ==");
            run_shared(&dev, None, Duration::from_secs(3), "default");
            println!("== shared, engine minimum period ==");
            let mut client = dev.get_iaudioclient().unwrap();
            let mix = client.get_mixformat().unwrap();
            let p = client.get_shared_mode_engine_period(&mix).unwrap();
            for (frames, tag) in [
                (p.min, "min"),
                (p.fundamental * 2, "2x fundamental"),
                (p.default, "default"),
            ] {
                println!("-- period {frames} frames ({tag}) --");
                run_shared(&dev, Some(frames), Duration::from_secs(3), tag);
            }
        }
        "exclusive" => {
            run_exclusive(&dev, Duration::from_secs(3));
        }
        _ => println!("usage: wasapi3-probe [devices|periods|shared|exclusive] [device-name-filter]"),
    }
}
