// Does a just-in-time feed policy hold low latency during continuous playback?
//
// The starve test showed publish->DAC falls to ~5.6ms when our own queue is
// empty (padding 0). During continuous playback we must still feed the device,
// so the question is how much queue depth is actually needed.
//
// Tests three steady-state policies, all measured while "playing" a continuous
// stream, with a publish (user clicks another row) every 60ms:
//   Full   - write all free space (queue fills to the endpoint buffer)
//   Period - write exactly one engine period per callback (queue stays ~0)
//   Half   - write half a period (starves periodically, shows the cost)
//
// Also reports underruns (callbacks where padding hit 0 while we owed audio).
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

#[derive(Clone, Copy, Debug)]
enum Feed {
    Full,
    Period,
    HalfPeriod,
}

fn run(dev: &Device, feed: Feed) -> Option<(f64, f64, u64, u64)> {
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
    // The engine period is not exposed by StreamMode::EventsShared here; the
    // endpoint buffer is two periods on this driver family, so half of it is
    // the period. Verified against the IAudioClient3 probe output.
    let period = buf / 2;
    let ev = client.set_get_eventhandle().unwrap();
    let render = client.get_audiorenderclient().unwrap();

    // Start empty on purpose, so we measure the steady state of each policy.
    client.start_stream().ok()?;

    let sh = Arc::new(Shared {
        pub_ns: AtomicU64::new(0),
        pub_seq: AtomicU64::new(0),
        t0: Instant::now(),
    });
    let sh2 = sh.clone();
    let pub_th = thread::spawn(move || {
        for _ in 0..40 {
            thread::sleep(Duration::from_millis(60));
            sh2.publish();
        }
    });

    let mut lats = Vec::new();
    let mut pads = Vec::new();
    let mut last_seen = 0u64;
    let mut underruns = 0u64;
    let mut callbacks = 0u64;
    let deadline = Instant::now() + Duration::from_millis(2500);

    while Instant::now() < deadline {
        if ev.wait_for_event(200).is_err() {
            break;
        }
        callbacks += 1;
        let pad = client.get_current_padding().unwrap();
        let space = client.get_available_space_in_frames().unwrap();
        if pad == 0 && callbacks > 2 {
            underruns += 1;
        }
        let seq = sh.pub_seq.load(Ordering::Acquire);
        if seq != last_seen {
            let w = (sh.now_ns().saturating_sub(sh.pub_ns.load(Ordering::Relaxed))) as f64 / 1e6;
            let q = pad as f64 / sr as f64 * 1000.0;
            lats.push(w + q);
            pads.push(pad);
            last_seen = seq;
        }
        if space == 0 {
            continue;
        }
        let n = match feed {
            Feed::Full => space,
            Feed::Period => space.min(period),
            Feed::HalfPeriod => space.min(period / 2),
        };
        render
            .write_to_device(&vec![0u8; n as usize * bpf], None)
            .unwrap();
    }
    let _ = pub_th.join();
    let _ = client.stop_stream();

    lats.sort_by(|a, b| a.partial_cmp(b).unwrap());
    pads.sort();
    println!(
        "  feed {:?}: endpoint={} period={} frames | callbacks={} underrun_callbacks={} | \
         padding median={} max={} | publish->DAC n={} median={:.2} p90={:.2} max={:.2}",
        feed,
        buf,
        period,
        callbacks,
        underruns,
        pads[pads.len() / 2],
        pads[pads.len() - 1],
        lats.len(),
        pct(&lats, 0.5),
        pct(&lats, 0.9),
        lats[lats.len() - 1]
    );
    Some((pct(&lats, 0.5), pct(&lats, 0.9), underruns, callbacks))
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
    for f in [Feed::Full, Feed::Period, Feed::HalfPeriod] {
        run(&dev, f);
    }
}
