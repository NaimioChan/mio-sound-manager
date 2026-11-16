// Exclusive-mode format probe: the mix format (32-bit float) is rejected in
// exclusive mode, so try the integer formats a device actually streams.
use std::time::{Duration, Instant};
use wasapi::*;

fn try_format(dev: &Device, bits: usize, valid_bits: usize, st: SampleType, rate: usize, ch: usize) {
    let label = format!("{bits}/{valid_bits}bit {:?} {rate}Hz {ch}ch", st);
    let fmt = WaveFormat::new(bits, valid_bits, st, rate, ch, None);
    let mut client = dev.get_iaudioclient().unwrap();
    match client.is_supported(&fmt, ShareMode::Exclusive) {
        Ok(None) => println!("  {label}: supported"),
        Ok(Some(m)) => {
            println!(
                "  {label}: not supported, nearest {}bit {}ch {}Hz",
                m.get_bitspersample(),
                m.get_nchannels(),
                m.get_samplespersec()
            );
            return;
        }
        Err(e) => {
            println!("  {label}: is_supported error {e}");
            return;
        }
    }

    let (_def_hns, min_hns) = client.get_device_period().unwrap();
    let mode = StreamMode::EventsExclusive { period_hns: min_hns };
    if let Err(e) = client.initialize_client(&fmt, Direction::Render, mode, None) {
        println!("  {label}: init failed {e}");
        return;
    }
    let blockalign = fmt.get_blockalign() as usize;
    let buf_frames = client.get_buffer_size().unwrap();
    let ev = client.set_get_eventhandle().unwrap();
    let render = client.get_audiorenderclient().unwrap();
    let space = client.get_available_space_in_frames().unwrap();
    if space > 0 {
        render
            .write_to_device(&vec![0u8; space as usize * blockalign], None)
            .unwrap();
    }
    client.start_stream().unwrap();

    let t0 = Instant::now();
    let mut blocks = Vec::new();
    let mut gaps = Vec::new();
    let mut last = Instant::now();
    let deadline = t0 + Duration::from_secs(2);
    while Instant::now() < deadline {
        if ev.wait_for_event(200).is_err() {
            println!("  {label}: event timeout");
            break;
        }
        let now = Instant::now();
        gaps.push((now - last).as_secs_f64() * 1000.0);
        last = now;
        let space = client.get_available_space_in_frames().unwrap();
        if space == 0 {
            continue;
        }
        blocks.push(space);
        render
            .write_to_device(&vec![0u8; space as usize * blockalign], None)
            .unwrap();
    }
    let _ = client.stop_stream();
    blocks.sort();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = blocks.len();
    if n == 0 {
        println!("  {label}: no callbacks");
        return;
    }
    println!(
        "  {label}: OK  buf={buf_frames} frames  callbacks={n}  frames/cb median={} ({:.2}ms)  \
         interval median={:.2}ms max={:.2}ms  (device min period {:.2}ms)",
        blocks[n / 2],
        blocks[n / 2] as f64 / rate as f64 * 1000.0,
        gaps[gaps.len() / 2],
        gaps[gaps.len() - 1],
        min_hns as f64 / 1e4
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
    let c = dev.get_iaudioclient().unwrap();
    let mix = c.get_mixformat().unwrap();
    println!(
        "mix: {}Hz {}ch {}bit",
        mix.get_samplespersec(),
        mix.get_nchannels(),
        mix.get_bitspersample()
    );
    for (bits, valid, st) in [
        (16usize, 16usize, SampleType::Int),
        (24, 24, SampleType::Int),
        (32, 24, SampleType::Int),
        (32, 32, SampleType::Float),
    ] {
        for rate in [44100usize, 48000] {
            try_format(&dev, bits, valid, st, rate, 2);
        }
    }
}
