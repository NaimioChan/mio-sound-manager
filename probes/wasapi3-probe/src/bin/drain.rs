// Can shared mode's queue be drained? The engine keeps ~12ms queued ahead of
// the render callback. If the app writes less than the free space while idle,
// the queue should shrink toward one period and publish->DAC should drop.
// Tests three write policies side by side.
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

fn pct(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v[(((v.len() as f64 - 1.0) * p).round()) as usize]
}

fn report(label: &str, mut v: Vec<f64>) {
    if v.is_empty() {
        println!("    {label}: no samples");
        return;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "    {label}: n={} median={:.2} min={:.2} p90={:.2} max={:.2}",
        v.len(),
        pct(&v, 0.5),
        v[0],
        pct(&v, 0.9),
        v[v.len() - 1]
    );
}

#[derive(Clone, Copy, Debug)]
enum Policy {
    /// Write the whole free space every callback (what a naive engine does).
    FillAll,
    /// While idle, top up only to one period; on publish, fill everything.
    DrainIdle,
    /// While idle write nothing at all; on publish fill everything.
    StarveIdle,
}

fn run(dev: &Device, policy: Policy) -> Option<(f64, f64)> {
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
        .ok()?;
    let buf = client.get_buffer_size().unwrap();
    let ev = client.set_get_eventhandle().unwrap();
    let render = client.get_audiorenderclient().unwrap();
    let space = client.get_available_space_in_frames().unwrap();
    if space > 0 {
        render
            .write_to_device(&vec![0u8; space as usize * bpf], None)
            .ok()?;
    }
    client.start_stream().ok()?;

    let sh = Arc::new(Shared {
        pub_ns: AtomicU64::new(0),
        pub_seq: AtomicU64::new(0),
        t0: Instant::now(),
    });
    let sh2 = sh.clone();
    let pub_th = thread::spawn(move || {
        for _ in 0..25 {
            thread::sleep(Duration::from_millis(60));
            sh2.publish();
        }
    });

    let period_frames = buf / 2;
    let mut total = Vec::new();
    let mut pads = Vec::new();
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
        let pad = client.get_current_padding().unwrap();
        let seq = sh.pub_seq.load(Ordering::Acquire);
        let fresh = seq != last_seen;
        if fresh {
            let w = (sh.now_ns().saturating_sub(sh.pub_ns.load(Ordering::Relaxed))) as f64 / 1e6;
            let q = pad as f64 / sr as f64 * 1000.0;
            pads.push(q);
            total.push(w + q);
            last_seen = seq;
        }

        let write_frames = match policy {
            Policy::FillAll => space,
            Policy::DrainIdle => {
                if fresh {
                    space
                } else {
                    space.min(period_frames)
                }
            }
            Policy::StarveIdle => {
                if fresh {
                    space
                } else {
                    0
                }
            }
        };
        if write_frames > 0 {
            render
                .write_to_device(&vec![0u8; write_frames as usize * bpf], None)
                .unwrap();
        }
    }
    let _ = pub_th.join();
    let _ = client.stop_stream();

    println!("  policy {:?} (endpoint buffer {} frames / {:.1}ms)", policy, buf, buf as f64 / sr as f64 * 1000.0);
    report("publish->DAC", total.clone());
    report("  queued ahead", pads);
    total.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some((pct(&total, 0.5), pct(&total, 0.9)))
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
                if d.get_friendlyname().unwrap().to_lowercase().contains(&f.to_lowercase()) {
                    found = Some(d);
                    break;
                }
            }
            found.expect("no matching device")
        }
    };
    println!("device: {:?}", dev.get_friendlyname().unwrap());
    for p in [Policy::FillAll, Policy::DrainIdle, Policy::StarveIdle] {
        run(&dev, p);
    }
}
