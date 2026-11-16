// Is the 10ms shared-mode engine period format-dependent?
// GetSharedModeEnginePeriod takes a format; a smaller min may be available for
// some formats. Sweeps bit depth / rate / channel count on every render device.
use wasapi::*;

fn main() {
    initialize_mta().unwrap();
    let en = DeviceEnumerator::new().unwrap();
    for d in &en.get_device_collection(Direction::Render).unwrap() {
        let d = d.unwrap();
        let name = d.get_friendlyname().unwrap();
        if !matches!(d.get_state().map(|s| format!("{s:?}")), Ok(ref s) if s == "Active") {
            continue;
        }
        println!("\n=== {name} ===");
        for (bits, valid, st, label) in [
            (32usize, 32usize, SampleType::Float, "f32"),
            (16, 16, SampleType::Int, "i16"),
            (32, 24, SampleType::Int, "i24in32"),
        ] {
            for rate in [44100usize, 48000, 96000] {
                for ch in [2usize] {
                    let fmt = WaveFormat::new(bits, valid, st, rate, ch, None);
                    let client = d.get_iaudioclient().unwrap();
                    // Only ask about formats the device actually accepts.
                    match client.is_supported(&fmt, ShareMode::Shared) {
                        Ok(Some(_)) => {
                            println!("  {label} {rate}Hz {ch}ch: not supported in shared mode");
                            continue;
                        }
                        Ok(None) => {}
                        Err(_) => {
                            println!("  {label} {rate}Hz {ch}ch: is_supported error");
                            continue;
                        }
                    }
                    match client.get_shared_mode_engine_period(&fmt) {
                        Ok(p) => {
                            let ms = |f: u32| f as f64 / rate as f64 * 1000.0;
                            println!(
                                "  {label} {rate}Hz {ch}ch: default={} ({:.2}ms) fundamental={} \
                                 min={} ({:.2}ms) max={} ({:.2}ms)",
                                p.default,
                                ms(p.default),
                                p.fundamental,
                                p.min,
                                ms(p.min),
                                p.max,
                                ms(p.max)
                            );
                        }
                        Err(e) => println!("  {label} {rate}Hz {ch}ch: engine period error {e}"),
                    }
                }
            }
        }
    }
}
