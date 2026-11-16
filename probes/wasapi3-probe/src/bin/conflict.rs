// Two questions the architecture depends on.
//
// A) Does AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM add queue depth? The previous
//    probes used autoconvert: true. If the engine inserts a resampler, that
//    resampler owns buffering we cannot see or drain.
//
// B) What happens to other clients when we hold exclusive mode? A sample
//    manager that silences the user's DAW is not shippable, so the conflict
//    behaviour has to be known rather than assumed.
use std::time::{Duration, Instant};

use wasapi::*;

fn measure_shared(dev: &Device, autoconvert: bool, label: &str) {
    let mut client = dev.get_iaudioclient().unwrap();
    let mix = client.get_mixformat().unwrap();
    let sr = mix.get_samplespersec();
    let bpf = mix.get_blockalign() as usize;
    let mode = StreamMode::EventsShared {
        autoconvert,
        buffer_duration_hns: 0,
    };
    if let Err(e) = client.initialize_client(&mix, Direction::Render, mode, None) {
        println!("  [{label}] init failed: {e}");
        return;
    }
    let buf = client.get_buffer_size().unwrap();
    let ev = client.set_get_eventhandle().unwrap();
    let render = client.get_audiorenderclient().unwrap();
    let space = client.get_available_space_in_frames().unwrap();
    if space > 0 {
        render
            .write_to_device(&vec![0u8; space as usize * bpf], None)
            .unwrap();
    }
    client.start_stream().unwrap();

    let mut pads = Vec::new();
    let mut spaces = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(1200);
    while Instant::now() < deadline {
        if ev.wait_for_event(200).is_err() {
            break;
        }
        let pad = client.get_current_padding().unwrap();
        let space = client.get_available_space_in_frames().unwrap();
        pads.push(pad);
        spaces.push(space);
        if space > 0 {
            render
                .write_to_device(&vec![0u8; space as usize * bpf], None)
                .unwrap();
        }
    }
    let _ = client.stop_stream();
    pads.sort();
    spaces.sort();
    let n = pads.len();
    if n == 0 {
        println!("  [{label}] no callbacks");
        return;
    }
    let ms = |f: u32| f as f64 / sr as f64 * 1000.0;
    println!(
        "  [{label}] endpoint={} frames ({:.1}ms) | padding median={} ({:.2}ms) | \
         free space median={} ({:.2}ms)",
        buf,
        ms(buf),
        pads[n / 2],
        ms(pads[n / 2]),
        spaces[n / 2],
        ms(spaces[n / 2])
    );
}

fn main() {
    initialize_mta().unwrap();
    let en = DeviceEnumerator::new().unwrap();

    // Find the real USB interface if it is present, else the default device.
    let mut dev = en.get_default_device(Direction::Render).unwrap();
    for d in &en.get_device_collection(Direction::Render).unwrap() {
        let d = d.unwrap();
        if d.get_friendlyname().unwrap().to_lowercase().contains("e2x2") {
            dev = d;
            break;
        }
    }
    println!("device: {:?}\n", dev.get_friendlyname().unwrap());

    println!("== A) autoconvert on vs off ==");
    measure_shared(&dev, true, "autoconvert=true");
    measure_shared(&dev, false, "autoconvert=false");

    println!("\n== B) exclusive held -> what breaks for other clients ==");
    // Take exclusive on 16-bit, the format this device accepts.
    let fmt = WaveFormat::new(16, 16, SampleType::Int, 44100, 2, None);
    let mut holder = dev.get_iaudioclient().unwrap();
    let (_, min_hns) = holder.get_device_period().unwrap();
    match holder.initialize_client(
        &fmt,
        Direction::Render,
        StreamMode::EventsExclusive { period_hns: min_hns },
        None,
    ) {
        Ok(()) => println!("  we hold exclusive: OK"),
        Err(e) => {
            println!("  we could not take exclusive: {e}");
            return;
        }
    }
    let bpf = fmt.get_blockalign() as usize;
    match holder.get_available_space_in_frames() {
        Ok(space) if space > 0 => {
            let render = holder.get_audiorenderclient().unwrap();
            render
                .write_to_device(&vec![0u8; space as usize * bpf], None)
                .unwrap();
        }
        Ok(_) => println!("  no free space after exclusive init"),
        Err(e) => println!("  get_available_space after exclusive init failed: {e}"),
    }
    if let Err(e) = holder.start_stream() {
        println!("  start_stream failed: {e}");
    }

    // A second exclusive client.
    let mut second = dev.get_iaudioclient().unwrap();
    match second.initialize_client(
        &fmt,
        Direction::Render,
        StreamMode::EventsExclusive { period_hns: min_hns },
        None,
    ) {
        Ok(()) => println!("  second exclusive client: OK (device is multi-client)"),
        Err(e) => println!("  second exclusive client: REFUSED ({e})"),
    }

    // A shared client, which is what a browser/DAW-adjacent app would use.
    let mut shared_client = dev.get_iaudioclient().unwrap();
    let mix = shared_client.get_mixformat().unwrap();
    match shared_client.initialize_client(
        &mix,
        Direction::Render,
        StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 0,
        },
        None,
    ) {
        Ok(()) => println!("  shared client while we hold exclusive: OK (would mix in)"),
        Err(e) => println!("  shared client while we hold exclusive: REFUSED ({e}) -> other apps go silent"),
    }

    let _ = holder.stop_stream();
    println!("\nreleased exclusive");
}
