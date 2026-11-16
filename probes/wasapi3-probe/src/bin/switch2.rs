// The switch path that actually discards queued audio: Stop -> Reset -> Start.
//
// Reset() is refused while the stream runs (AUDCLNT_E_NOT_STOPPED,
// 0x88890005). If Stop+Reset+Start is cheap, WASAPI shared mode can do an
// instant audition switch: the queued old audio is dropped, the new file's
// first sample leaves on the next period instead of after ~12ms of stale audio.
//
// Measures, per switch:
//   - cost of Stop+Reset+Start (wall clock, on the calling thread)
//   - publish -> first new sample reaching the DAC afterwards
//   - whether the stream stays healthy across many switches
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

    let sh = Arc::new(Shared {
        pub_ns: AtomicU64::new(0),
        pub_seq: AtomicU64::new(0),
        t0: Instant::now(),
    });

    // Render thread: keeps the stream fed, and performs the switch when asked.
    let sh_r = sh.clone();
    let stop = Arc::new(AtomicU64::new(0));
    let stop_r = stop.clone();
    let name = dev.get_friendlyname().unwrap();
    let handle = thread::spawn(move || {
        // AudioClient is !Send, so it is created on this thread.
        let en = DeviceEnumerator::new().unwrap();
        let mut dev = en.get_default_device(Direction::Render).unwrap();
        for d in &en.get_device_collection(Direction::Render).unwrap() {
            let d = d.unwrap();
            if d.get_friendlyname().unwrap() == name {
                dev = d;
                break;
            }
        }
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
        let render = client.get_audiorenderclient().unwrap();
        // The Handle must outlive the stream: dropping it closes the event, and
        // Start() then fails with AUDCLNT_E_EVENTHANDLE_NOT_SET.
        // Keep the handle alive for the stream's lifetime; Reset() clears it.
        let mut _ev = Some(client.set_get_eventhandle().unwrap());
        let mut lats = Vec::new();
        let mut costs = Vec::new();
        let mut pads = Vec::new();
        let mut last_seen = 0u64;
        let mut errors: Vec<String> = Vec::new();
        let mut started = false;
        // Start with the stream running and a primed buffer.
        loop {
            if stop_r.load(Ordering::Relaxed) == 1 && sh_r.pub_seq.load(Ordering::Relaxed) == last_seen {
                break;
            }
            let seq = sh_r.pub_seq.load(Ordering::Acquire);
            if seq != last_seen {
                // Switch: discard queued audio, then restart.
                let t = Instant::now();
                if started {
                    let _ = client.stop_stream();
                    if let Err(e) = client.reset_stream() {
                        errors.push(format!("reset: {e}"));
                    }
                    // Reset() drops the registered event; re-register and keep
                    // the Handle alive for as long as the stream runs.
                    match client.set_get_eventhandle() {
                        Ok(h) => _ev = Some(h),
                        Err(e) => errors.push(format!("setevent: {e}")),
                    }
                }
                if let Err(e) = client.start_stream() {
                    errors.push(format!("start: {e}"));
                }
                started = true;
                let cost = t.elapsed().as_secs_f64() * 1000.0;
                costs.push(cost);

                // Write one block immediately; the queue is now empty.
                let pad = client.get_current_padding().unwrap_or(9999);
                pads.push(pad);
                let space = client.get_available_space_in_frames().unwrap_or(0);
                if space > 0 {
                    if let Err(e) =
                        render.write_to_device(&vec![0u8; space as usize * bpf], None)
                    {
                        errors.push(format!("write: {e}"));
                    }
                }
                let w = (sh_r.now_ns().saturating_sub(sh_r.pub_ns.load(Ordering::Relaxed))) as f64 / 1e6;
                lats.push(w + pad as f64 / sr as f64 * 1000.0);
                last_seen = seq;
            } else {
                // Steady state: keep the buffer topped up, but only when there
                // is real space, so we never build a long queue.
                if started {
                    if let Ok(space) = client.get_available_space_in_frames() {
                        if space > 0 {
                            let _ = render
                                .write_to_device(&vec![0u8; space as usize * bpf], None);
                        }
                    }
                }
                thread::sleep(Duration::from_millis(1));
            }
        }
        let _ = client.stop_stream();
        (lats, costs, pads, errors, sr)
    });

    // Publisher: simulate the user arrowing through rows, with pauses between
    // so the stream goes idle (the realistic audition pattern).
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(120));
        sh.publish();
    }
    thread::sleep(Duration::from_millis(200));
    stop.store(1, Ordering::Relaxed);
    sh.publish();
    let (lats, costs, pads, errors, sr) = handle.join().unwrap();

    println!("switches: {}", lats.len());
    report("Stop+Reset+Start cost", costs);
    report("publish->DAC after switch", lats);
    let mut p = pads.clone();
    p.sort();
    if !p.is_empty() {
        println!(
            "  padding immediately after restart: median={} frames ({:.2}ms) max={}",
            p[p.len() / 2],
            p[p.len() / 2] as f64 / sr as f64 * 1000.0,
            p[p.len() - 1]
        );
    }
    if !errors.is_empty() {
        println!("  errors ({}): {:?}", errors.len(), &errors[..errors.len().min(6)]);
    }
}
