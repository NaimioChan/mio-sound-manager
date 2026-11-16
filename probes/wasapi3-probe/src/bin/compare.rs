// Same honest measurement, but for exclusive mode (which needs an integer
// format, not the float mix format) and for ASIO if built in.
//
// Reports publish -> first-sample-reaches-DAC, counting the queue ahead.
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

fn pick(filter: Option<&str>) -> Device {
    let en = DeviceEnumerator::new().unwrap();
    match filter {
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
            found.expect("no matching device")
        }
    }
}

/// Shared mode with an explicit requested buffer duration (hns).
fn shared(dev: &Device, buffer_hns: i64, tag: &str) {
    let mut client = dev.get_iaudioclient().unwrap();
    let mix = client.get_mixformat().unwrap();
    let sr = mix.get_samplespersec();
    let bpf = mix.get_blockalign() as usize;
    if let Err(e) = client.initialize_client(
        &mix,
        Direction::Render,
        StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: buffer_hns,
        },
        None,
    ) {
        println!("  [shared {tag}] init failed: {e}");
        return;
    }
    let buf = client.get_buffer_size().unwrap();
    let ev = client.set_get_eventhandle().unwrap();
    let render = client.get_audiorenderclient().unwrap();
    println!(
        "  [shared {tag}] requested buffer {}hns -> endpoint buffer {} frames ({:.2}ms)",
        buffer_hns,
        buf,
        buf as f64 / sr as f64 * 1000.0
    );
    let space = client.get_available_space_in_frames().unwrap();
    if space > 0 {
        render
            .write_to_device(&vec![0u8; space as usize * bpf], None)
            .unwrap();
    }
    client.start_stream().unwrap();

    let sh = Arc::new(Shared {
        pub_ns: AtomicU64::new(0),
        pub_seq: AtomicU64::new(0),
        t0: Instant::now(),
    });
    let sh2 = sh.clone();
    let pub_th = thread::spawn(move || {
        for _ in 0..30 {
            thread::sleep(Duration::from_millis(50));
            sh2.publish();
        }
    });

    let mut total = Vec::new();
    let mut queue = Vec::new();
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
        if seq != last_seen {
            let w = (sh.now_ns().saturating_sub(sh.pub_ns.load(Ordering::Relaxed))) as f64 / 1e6;
            let q = pad as f64 / sr as f64 * 1000.0;
            queue.push(q);
            total.push(w + q);
            last_seen = seq;
        }
        render
            .write_to_device(&vec![0u8; space as usize * bpf], None)
            .unwrap();
    }
    let _ = pub_th.join();
    let _ = client.stop_stream();
    report(&format!("[shared {tag}] publish->DAC"), total);
    report(&format!("[shared {tag}]   queued ahead"), queue);
}

fn exclusive(dev: &Device, bits: usize, valid: usize, st: SampleType, rate: usize, tag: &str) {
    let fmt = WaveFormat::new(bits, valid, st, rate, 2, None);
    let mut client = dev.get_iaudioclient().unwrap();
    if !matches!(client.is_supported(&fmt, ShareMode::Exclusive), Ok(None)) {
        println!("  [exclusive {tag}] format unsupported");
        return;
    }
    let (_, min_hns) = client.get_device_period().unwrap();
    if let Err(e) = client.initialize_client(
        &fmt,
        Direction::Render,
        StreamMode::EventsExclusive { period_hns: min_hns },
        None,
    ) {
        println!("  [exclusive {tag}] init failed: {e}");
        return;
    }
    let bpf = fmt.get_blockalign() as usize;
    let buf = client.get_buffer_size().unwrap();
    let ev = client.set_get_eventhandle().unwrap();
    let render = client.get_audiorenderclient().unwrap();
    println!(
        "  [exclusive {tag}] period {}hns -> buffer {} frames ({:.2}ms)",
        min_hns,
        buf,
        buf as f64 / rate as f64 * 1000.0
    );
    let space = client.get_available_space_in_frames().unwrap();
    if space > 0 {
        render
            .write_to_device(&vec![0u8; space as usize * bpf], None)
            .unwrap();
    }
    client.start_stream().unwrap();

    let sh = Arc::new(Shared {
        pub_ns: AtomicU64::new(0),
        pub_seq: AtomicU64::new(0),
        t0: Instant::now(),
    });
    let sh2 = sh.clone();
    let pub_th = thread::spawn(move || {
        for _ in 0..30 {
            thread::sleep(Duration::from_millis(50));
            sh2.publish();
        }
    });

    let mut total = Vec::new();
    let mut queue = Vec::new();
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
        if seq != last_seen {
            let w = (sh.now_ns().saturating_sub(sh.pub_ns.load(Ordering::Relaxed))) as f64 / 1e6;
            let q = pad as f64 / rate as f64 * 1000.0;
            queue.push(q);
            total.push(w + q);
            last_seen = seq;
        }
        render
            .write_to_device(&vec![0u8; space as usize * bpf], None)
            .unwrap();
    }
    let _ = pub_th.join();
    let _ = client.stop_stream();
    report(&format!("[exclusive {tag}] publish->DAC"), total);
    report(&format!("[exclusive {tag}]   queued ahead"), queue);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let filter: Option<&str> = args.get(1).map(|s| s.as_str()).filter(|s| *s != "-");
    initialize_mta().unwrap();
    let dev = pick(filter);
    println!("device: {:?}", dev.get_friendlyname().unwrap());

    println!("== shared, varying requested buffer ==");
    for (hns, tag) in [
        (0i64, "engine default"),
        (50_000, "5ms"),
        (100_000, "10ms"),
        (200_000, "20ms"),
    ] {
        shared(&dev, hns, tag);
    }

    println!("== exclusive, min device period ==");
    exclusive(&dev, 16, 16, SampleType::Int, 44100, "16bit 44k1");
    exclusive(&dev, 32, 24, SampleType::Int, 44100, "24in32 44k1");
}
