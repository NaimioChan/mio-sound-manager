// Honest latency probe: "publish a clip" -> "that clip's first sample reaches
// the DAC", not just "the render callback noticed it".
//
// The earlier probe stopped the clock when the callback wrote the block. But a
// block written at callback N still has to wait for whatever is already queued
// ahead of it in the endpoint buffer. This probe reports that queue depth
// (get_current_padding) at write time, so the number includes it.
//
// Keeps padding minimal (writes only the free space each callback) because that
// is what a real playback engine does.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use wasapi::*;

struct Shared {
    pub_ns: AtomicU64,
    pub_seq: AtomicU64,
    t0: Instant,
}

impl Shared {
    fn now_ns(&self) -> u64 {
        self.t0.elapsed().as_nanos() as u64
    }
    fn publish(&self) {
        let ns = self.now_ns();
        self.pub_ns.store(ns, Ordering::Relaxed);
        self.pub_seq.fetch_add(1, Ordering::Release);
    }
}

fn pct(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let i = ((v.len() as f64 - 1.0) * p).round() as usize;
    v[i]
}

fn report(label: &str, mut v: Vec<f64>) {
    if v.is_empty() {
        println!("  {label}: no samples");
        return;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "  {label}: n={} median={:.2}ms min={:.2} p90={:.2} max={:.2}",
        v.len(),
        pct(&v, 0.5),
        v[0],
        pct(&v, 0.9),
        v[v.len() - 1]
    );
}


/// Device names are matched by substring. Print what exists when nothing matches,
/// so a wrong name is an actionable error instead of a bare panic.
fn device_names(en: &DeviceEnumerator) -> String {
    let mut names = Vec::new();
    if let Ok(coll) = en.get_device_collection(Direction::Render) {
        for d in &coll {
            if let Ok(d) = d {
                if let Ok(n) = d.get_friendlyname() {
                    names.push(n);
                }
            }
        }
    }
    names.join(" | ")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let filter: Option<&str> = args.get(1).map(|s| s.as_str()).filter(|s| *s != "-");
    initialize_mta().unwrap();

    let en = DeviceEnumerator::new().unwrap();
    let dev = match filter {
        None => en.get_default_device(Direction::Render).unwrap(),
        Some(f) => {
            let mut found = None;
            for d in &en.get_device_collection(Direction::Render).unwrap() {
                let d = d.unwrap();
                if d.get_friendlyname()
                    .unwrap()
                    .to_lowercase()
                    .contains(&f.to_lowercase())
                {
                    found = Some(d);
                    break;
                }
            }
            found.unwrap_or_else(|| {
                panic!(
                    "no render device matching {f:?}. Available: {}",
                    device_names(&en)
                )
            })
        }
    };
    println!("device: {:?}", dev.get_friendlyname().unwrap());

    let mut client = dev.get_iaudioclient().unwrap();
    let mix = client.get_mixformat().unwrap();
    let sr = mix.get_samplespersec();
    let ch = mix.get_nchannels();
    let blockalign = mix.get_blockalign() as usize;
    let bytes_per_frame = blockalign;

    // Shared mode, engine default period, event driven.
    client
        .initialize_client(
            &mix,
            Direction::Render,
            StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: 0,
            },
            None,
        )
        .unwrap();
    let buf_frames = client.get_buffer_size().unwrap();
    let ev = client.set_get_eventhandle().unwrap();
    let render = client.get_audiorenderclient().unwrap();
    println!(
        "shared {}Hz {}ch, endpoint buffer = {} frames ({:.2}ms)",
        sr,
        ch,
        buf_frames,
        buf_frames as f64 / sr as f64 * 1000.0
    );

    // Prime with silence, then start.
    let space = client.get_available_space_in_frames().unwrap();
    if space > 0 {
        render
            .write_to_device(&vec![0u8; space as usize * bytes_per_frame], None)
            .unwrap();
    }
    client.start_stream().unwrap();

    let sh = Arc::new(Shared {
        pub_ns: AtomicU64::new(0),
        pub_seq: AtomicU64::new(0),
        t0: Instant::now(),
    });

    let sh2 = sh.clone();
    let publisher = thread::spawn(move || {
        for _ in 0..30 {
            thread::sleep(Duration::from_millis(50));
            sh2.publish();
        }
    });

    let mut write_lat = Vec::new();
    let mut queue_ms = Vec::new();
    let mut audible_ms = Vec::new();
    let mut padding_samples = Vec::new();
    let mut last_seen = 0u64;
    let deadline = Instant::now() + Duration::from_millis(1700);

    while Instant::now() < deadline {
        if ev.wait_for_event(200).is_err() {
            break;
        }
        let space = client.get_available_space_in_frames().unwrap();
        if space == 0 {
            continue;
        }
        let padding = client.get_current_padding().unwrap();
        padding_samples.push(padding as f64);

        let seq = sh.pub_seq.load(Ordering::Acquire);
        if seq != last_seen {
            let pub_ns = sh.pub_ns.load(Ordering::Relaxed);
            let write_ns = sh.now_ns();
            let w = (write_ns.saturating_sub(pub_ns)) as f64 / 1e6;
            // What is already queued ahead of the block we are about to write.
            let q = padding as f64 / sr as f64 * 1000.0;
            write_lat.push(w);
            queue_ms.push(q);
            audible_ms.push(w + q);
            last_seen = seq;
        }

        // Write only the free space, so the queue stays as short as possible.
        render
            .write_to_device(&vec![0u8; space as usize * bytes_per_frame], None)
            .unwrap();
    }
    let _ = publisher.join();
    let _ = client.stop_stream();

    padding_samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "  queue depth at write: median {} frames ({:.2}ms), max {} frames ({:.2}ms)",
        padding_samples[padding_samples.len() / 2],
        padding_samples[padding_samples.len() / 2] / sr as f64 * 1000.0,
        padding_samples[padding_samples.len() - 1],
        padding_samples[padding_samples.len() - 1] / sr as f64 * 1000.0
    );
    report("publish -> callback writes it", write_lat);
    report("  of which already queued ahead", queue_ms);
    report("publish -> first sample reaches DAC (est.)", audible_ms);
}
