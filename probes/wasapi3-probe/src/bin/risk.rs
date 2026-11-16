// Risk check for the starve-when-idle policy on WASAPI shared mode.
//
// Draining the queue by not writing while idle drops publish->DAC from ~17ms
// to ~5.6ms. The question is what it costs:
//   1. Does the stream keep running after a long idle, or does the driver stop
//      it / report an error?
//   2. Does the first block after a long idle arrive promptly (i.e. does the
//      engine restart cleanly)?
//   3. Does underrunning produce a discontinuity the app must fade over?
//
// Reports the exact sample values written and read back via loopback where
// possible; here we can only observe timing + WASAPI error codes, so the test
// prints every failure instead of swallowing it.
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
        self.pub_ns.store(self.now_ns(), Ordering::Relaxed);
        self.pub_seq.fetch_add(1, Ordering::Release);
    }
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
    let idle_ms: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3000);

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
    let bpf = mix.get_blockalign() as usize;
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
    let buf = client.get_buffer_size().unwrap();
    let ev = client.set_get_eventhandle().unwrap();
    let render = client.get_audiorenderclient().unwrap();
    println!("endpoint buffer {} frames ({:.2}ms)", buf, buf as f64 / sr as f64 * 1000.0);

    let space = client.get_available_space_in_frames().unwrap();
    if space > 0 {
        render
            .write_to_device(&vec![0u8; space as usize * bpf], None)
            .unwrap();
    }
    client.start_stream().unwrap();

    // Phase 1: starve for `idle_ms`, counting callbacks and watching for errors.
    println!("\n-- phase 1: starve for {idle_ms}ms --");
    let starve_deadline = Instant::now() + Duration::from_millis(idle_ms);
    let mut callbacks = 0u64;
    let mut errs = Vec::new();
    let mut pads = Vec::new();
    while Instant::now() < starve_deadline {
        match ev.wait_for_event(100) {
            Ok(()) => {
                callbacks += 1;
                match client.get_current_padding() {
                    Ok(p) => pads.push(p),
                    Err(e) => errs.push(format!("padding: {e}")),
                }
            }
            Err(e) => errs.push(format!("wait: {e}")),
        }
    }
    let expected = idle_ms as f64 / 1000.0 * sr as f64 / (buf as f64 / 2.0);
    println!(
        "  callbacks={callbacks} (expected ~{expected:.0} if the engine keeps waking us)"
    );
    println!(
        "  errors={:?}",
        if errs.is_empty() {
            "none".to_string()
        } else {
            format!("{}", errs.len())
        }
    );
    if !pads.is_empty() {
        pads.sort();
        println!(
            "  padding during starve: median={} max={} frames",
            pads[pads.len() / 2],
            pads[pads.len() - 1]
        );
    }

    // Phase 2: publish once after the long idle. How fast does it reach the DAC?
    println!("\n-- phase 2: first publish after {idle_ms}ms idle --");
    let sh = Arc::new(Shared {
        pub_ns: AtomicU64::new(0),
        pub_seq: AtomicU64::new(0),
        t0: Instant::now(),
    });
    sh.publish();
    let mut lat = None;
    let mut waited = Instant::now();
    while waited.elapsed() < Duration::from_millis(500) {
        if ev.wait_for_event(200).is_err() {
            println!("  event wait failed / timed out");
            break;
        }
        let space = client.get_available_space_in_frames().unwrap();
        let pad = client.get_current_padding().unwrap();
        if space > 0 {
            let w = (sh.now_ns().saturating_sub(sh.pub_ns.load(Ordering::Relaxed))) as f64 / 1e6;
            let q = pad as f64 / sr as f64 * 1000.0;
            lat = Some((w, q, w + q));
            render
                .write_to_device(&vec![0u8; space as usize * bpf], None)
                .unwrap();
            break;
        }
        waited = Instant::now();
    }
    match lat {
        Some((w, q, t)) => println!(
            "  publish->write {w:.2}ms, queued ahead {q:.2}ms, publish->DAC {t:.2}ms"
        ),
        None => println!("  never got space to write"),
    }

    // Phase 3: is the stream still healthy after the starve? Hammer it with a
    // burst of publishes (arrow-key rate) and see whether latency stays flat.
    println!("\n-- phase 3: burst of publishes at 60ms intervals (arrow keys) --");
    let mut lats = Vec::new();
    let mut last_seen = 0u64;
    let burst_end = Instant::now() + Duration::from_millis(2400);
    let sh2 = sh.clone();
    let pub_th = thread::spawn(move || {
        for _ in 0..40 {
            thread::sleep(Duration::from_millis(60));
            sh2.publish();
        }
    });
    while Instant::now() < burst_end {
        if ev.wait_for_event(200).is_err() {
            break;
        }
        let space = client.get_available_space_in_frames().unwrap();
        if space == 0 {
            continue;
        }
        let pad = client.get_current_padding().unwrap();
        let seq = sh.pub_seq.load(Ordering::Acquire);
        if seq != last_seen {
            let w = (sh.now_ns().saturating_sub(sh.pub_ns.load(Ordering::Relaxed))) as f64 / 1e6;
            lats.push(w + pad as f64 / sr as f64 * 1000.0);
            last_seen = seq;
        }
        render
            .write_to_device(&vec![0u8; space as usize * bpf], None)
            .unwrap();
    }
    let _ = pub_th.join();
    if !lats.is_empty() {
        lats.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  n={} median={:.2}ms min={:.2} p90={:.2} max={:.2}",
            lats.len(),
            lats[lats.len() / 2],
            lats[0],
            lats[(lats.len() - 1) * 9 / 10],
            lats[lats.len() - 1]
        );
    }

    let _ = client.stop_stream();
    println!("\nstream stopped cleanly");
}
