//! ASIO output, behind the `asio` feature.
//!
//! The shape is deliberately the same as the WASAPI backend in `output.rs`: one
//! render thread that owns the device, a callback that copies from the shared
//! dual-slot pair and nothing else, and a blocking handshake so `Engine::start`
//! only returns once the stream is live and the format is known.
//!
//! What differs is who calls the callback. WASAPI blocks the render thread on an
//! event handle and we pull; ASIO pushes — the driver calls our callback on its
//! own thread whenever it wants the next buffer. So the thread here only opens
//! the driver, starts it, and then sleeps until it is told to stop; the audio
//! work happens on the driver's thread, inside `Mixer::fill`.
//!
//! Two constraints from the ASIO SDK shape the code:
//!
//! * Only one driver can be loaded per process, and `asio-sys` keeps a global
//!   registry of callbacks. So the callback is registered by id and removed on
//!   the way out; leaving it registered would hand the next engine a callback
//!   whose buffers no longer exist.
//! * Buffers are **non-interleaved**: one pointer per channel, two halves per
//!   channel for double buffering. The mixer produces interleaved bytes, so the
//!   callback de-interleaves into the channel planes. That is a copy, but it is
//!   the same order of work as the WASAPI write and it happens on a buffer of
//!   `buffer_size` frames rather than the whole clip.
//!
//! The Steinberg SDK is proprietary: it is not vendored here and `asio-sys`
//! fetches it at build time (`CPAL_ASIO_DIR` or `%TEMP%\asio_sdk`), and bindgen
//! needs a `libclang.dll` (`LIBCLANG_PATH`). `tools/build-app.sh` documents the
//! two variables.

use std::sync::Arc;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use asio_sys::{Asio, AsioSampleType, BufferPreference, Driver};

use crate::decode::{DeviceSample, OutputFormat};
use crate::error::{EngineError, Result};
use crate::output::{Mixer, Shared, StreamInfo, join_pro_audio_group};

/// One ASIO driver the host can open.
#[derive(Debug, Clone)]
pub struct AsioDevice {
    /// The driver's own name, which is what `load_driver` takes. ASIO has no
    /// stable id beyond this string.
    pub name: String,
    /// Input channels. Reported because a driver that only does inputs cannot be
    /// used as an output device, and saying so is better than an opaque failure.
    pub inputs: u32,
    pub outputs: u32,
    /// Preferred buffer size in frames, when the driver reports one.
    pub buffer_frames: Option<u32>,
    pub sample_rate: Option<u32>,
    /// Whether the driver is known to be usable. `false` only when a probe ran and
    /// failed — i.e. the interface is unplugged or its driver is broken. An
    /// unprobed driver is `true`: the registry lists it and nothing has shown it to
    /// be broken, which is the honest reading of "no evidence against it".
    pub available: bool,
}

/// The driver's preferred buffer size, whatever shape it reported.
fn preferred_frames(pref: BufferPreference) -> u32 {
    match pref {
        BufferPreference::Only(n) | BufferPreference::Preferred(n) => n,
        BufferPreference::Stepped { preferred, .. } => preferred,
    }
}

/// Every installed ASIO driver.
///
/// `probe` controls whether each driver is actually opened to read its channel
/// count and preferred buffer size. It has to be off while this process already
/// has a driver running: the SDK's `loadDriver` calls `removeCurrentDriver()`
/// first (see `host/asiodrivers.cpp`), so probing would close the live stream and
/// leave our callback registered against a released driver. Names are always
/// read from the registry, which costs nothing and cannot disturb playback.
///
/// Probing is also not free and not always safe: a driver whose hardware is
/// unplugged can block inside `ASIOInit` for seconds. A driver that fails to
/// probe is still listed with `available: false`, because "your interface is not
/// plugged in" is a useful thing for the settings dialog to say.
pub fn list_devices(probe: bool) -> Vec<AsioDevice> {
    // ASIO drivers are COM objects that only hand out their interface from a
    // single-threaded apartment (see `render_thread` for the measurements), and
    // the apartment of a thread cannot be changed once set. `output::list_devices`
    // enumerates WASAPI first, which puts the calling thread into MTA — so
    // enumerating ASIO on that same thread fails with `RPC_E_CHANGED_MODE` and
    // every driver looks absent.
    //
    // A dedicated STA thread makes the list independent of whatever the caller
    // did before. It costs one thread spawn per enumeration, which is a
    // settings-dialog open, not a hot path.
    let names: Vec<(String, Option<(u32, u32, Option<u32>, Option<u32>)>)> = match std::thread::Builder::new()
        .name("reslip-asio-enum".into())
        .spawn(move || {
            let _ = wasapi::initialize_sta();
            let asio = Asio::new();
            let names = asio.driver_names();
            let details = names
                .iter()
                .map(|name| probe.then(|| probe_driver(&asio, name)).flatten())
                .collect::<Vec<_>>();
            names.into_iter().zip(details).collect::<Vec<_>>()
        })
        .map(|handle| handle.join())
    {
        Ok(Ok(list)) => list,
        // No ASIO at all is a legitimate state, and a thread that would not spawn
        // is a broken process; neither should stop the WASAPI endpoints from
        // being listed.
        _ => return Vec::new(),
    };

    names
        .into_iter()
        .map(|(name, details)| match details {
            Some((inputs, outputs, buffer_frames, sample_rate)) => AsioDevice {
                name,
                inputs,
                outputs,
                buffer_frames,
                sample_rate,
                available: outputs > 0,
            },
            // Either not probed, or the probe failed. A name from the registry is
            // still worth listing in both cases; only a failed probe is evidence
            // that the driver cannot be opened.
            None => AsioDevice {
                name,
                inputs: 0,
                outputs: 0,
                buffer_frames: None,
                sample_rate: None,
                available: !probe,
            },
        })
        .collect()
}

fn probe_driver(asio: &Asio, name: &str) -> Option<(u32, u32, Option<u32>, Option<u32>)> {
    let driver = asio.load_driver(name).ok()?;
    let channels = driver.channels().ok()?;
    let buffer = driver
        .buffersize_range()
        .ok()
        .map(|range| preferred_frames(range.preferred));
    let rate = driver
        .sample_rate()
        .ok()
        .filter(|r| *r > 0.0)
        .map(|r| r as u32);
    // The driver is unloaded here: `Driver`'s `Drop` calls `ASIOExit`, and ASIO
    // allows exactly one loaded driver at a time, so holding it would make the
    // next probe fail.
    Some((channels.ins.max(0) as u32, channels.outs.max(0) as u32, buffer, rate))
}

/// Pick a driver by case-insensitive substring, or the first one when no filter
/// is given. Mirrors `output::pick_device`, including the error text.
fn pick_driver(asio: &Asio, filter: Option<&str>) -> Result<String> {
    let names = asio.driver_names();
    let Some(filter) = filter else {
        return names
            .into_iter()
            .next()
            .ok_or_else(|| EngineError::Audio("no ASIO driver is installed".into()));
    };

    let lower = filter.to_lowercase();
    let mut available = Vec::new();
    for name in names {
        if name.to_lowercase().contains(&lower) {
            return Ok(name);
        }
        available.push(name);
    }
    Err(EngineError::DeviceNotFound {
        filter: filter.to_string(),
        available: if available.is_empty() {
            "none".to_string()
        } else {
            available.join(" | ")
        },
    })
}

/// The ASIO sample type as the engine's own enum, or an error naming the layout
/// it cannot emit.
fn sample_of(kind: &AsioSampleType) -> Result<DeviceSample> {
    match kind {
        AsioSampleType::ASIOSTFloat32LSB | AsioSampleType::ASIOSTFloat32MSB => {
            Ok(DeviceSample::F32)
        }
        AsioSampleType::ASIOSTInt16LSB | AsioSampleType::ASIOSTInt16MSB => Ok(DeviceSample::I16),
        // The 24-in-32 and 32-bit-aligned layouts are a full 32-bit container
        // with the sample left-shifted into the top bits, so scaling by
        // `i32::MAX` is what a driver expects. The plain `ASIOSTInt32LSB` is the
        // same thing.
        AsioSampleType::ASIOSTInt32LSB
        | AsioSampleType::ASIOSTInt32MSB
        | AsioSampleType::ASIOSTInt32LSB16
        | AsioSampleType::ASIOSTInt32LSB18
        | AsioSampleType::ASIOSTInt32LSB20
        | AsioSampleType::ASIOSTInt32LSB24
        | AsioSampleType::ASIOSTInt32MSB16
        | AsioSampleType::ASIOSTInt32MSB18
        | AsioSampleType::ASIOSTInt32MSB20
        | AsioSampleType::ASIOSTInt32MSB24 => Ok(DeviceSample::I32),
        other => Err(EngineError::Unsupported(format!(
            "ASIO driver wants {other:?}, which this engine cannot write"
        ))),
    }
}

/// Whether the driver's output data is big-endian. The sample bytes the decoder
/// produces are little-endian, so a big-endian driver needs a swap per sample.
fn is_big_endian(kind: &AsioSampleType) -> bool {
    matches!(
        kind,
        AsioSampleType::ASIOSTFloat32MSB
            | AsioSampleType::ASIOSTFloat64MSB
            | AsioSampleType::ASIOSTInt16MSB
            | AsioSampleType::ASIOSTInt24MSB
            | AsioSampleType::ASIOSTInt32MSB
            | AsioSampleType::ASIOSTInt32MSB16
            | AsioSampleType::ASIOSTInt32MSB18
            | AsioSampleType::ASIOSTInt32MSB20
            | AsioSampleType::ASIOSTInt32MSB24
    )
}

/// Open the driver, start it, and block until `Shared::running` goes false.
pub(crate) fn render_thread(
    shared: Arc<Shared>,
    filter: Option<String>,
    requested_frames: Option<u32>,
    ready: SyncSender<Result<StreamInfo>>,
) {
    let _mmcss = join_pro_audio_group();

    // ASIO drivers are COM objects created with `CoCreateInstance` (see
    // `asioOpenDriver` in the SDK's `asiolist.cpp`), and the TOE driver behind
    // this machine's TOPPING interface only hands out its object from a
    // single-threaded apartment. Measured on this host: loading from the main
    // thread works (it is STA by default), from an MTA worker it fails with
    // `loadDriver returned false`, and from an STA worker it works.
    //
    // WASAPI wants the opposite (`initialize_mta`), so the two backends must not
    // share a thread: this thread is STA, the WASAPI one is MTA.
    let _ = wasapi::initialize_sta();

    let opened = open_stream(&shared, filter.as_deref(), requested_frames);
    let (info, driver, callback_id) = match opened {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };

    if let Err(e) = driver.start() {
        driver.remove_callback(callback_id);
        let _ = ready.send(Err(EngineError::Audio(format!("ASIO start failed: {e}"))));
        return;
    }

    let _ = ready.send(Ok(info));

    // The audio work happens on the driver's thread. This one only waits for the
    // stop flag, because `ASIOStart` returns immediately and there is nothing
    // else to poll.
    while shared.is_running() {
        std::thread::sleep(Duration::from_millis(20));
    }

    // Order matters: stop the driver before removing the callback, or a buffer
    // switch can arrive between the two and find no callback. Stopping first is
    // what `asio-sys` does internally too.
    let _ = driver.stop();
    driver.remove_callback(callback_id);
}

type OpenStream = (StreamInfo, Arc<Driver>, asio_sys::BufferCallbackId);

fn open_stream(
    shared: &Arc<Shared>,
    filter: Option<&str>,
    requested_frames: Option<u32>,
) -> Result<OpenStream> {
    let asio = Asio::new();
    let name = pick_driver(&asio, filter)?;
    let driver = Arc::new(
        asio.load_driver(&name)
            .map_err(|e| EngineError::Audio(format!("cannot open ASIO driver {name:?}: {e}")))?,
    );

    let channels = driver
        .channels()
        .map_err(|e| EngineError::Audio(format!("ASIO channel query failed: {e}")))?;
    if channels.outs < 1 {
        return Err(EngineError::Audio(format!(
            "ASIO driver {name:?} has no output channels"
        )));
    }

    // The driver decides the rate; asking for one it cannot do fails, and asking
    // for the current one is a no-op. The engine resamples at decode time, so
    // whatever the driver reports is the format the whole app uses.
    let sample_rate = driver
        .sample_rate()
        .map_err(|e| EngineError::Audio(format!("ASIO sample rate query failed: {e}")))?;
    if !(sample_rate.is_finite() && sample_rate > 0.0) {
        return Err(EngineError::Audio(format!(
            "ASIO driver {name:?} reports an unusable sample rate ({sample_rate})"
        )));
    }
    let sample_rate = sample_rate as u32;

    let out_type = driver
        .output_data_type()
        .map_err(|e| EngineError::Audio(format!("ASIO data type query failed: {e}")))?;
    let sample = sample_of(&out_type)?;

    // Two channels is what a sample manager plays. A driver with more is opened
    // with just the first two, so a surround interface does not get fed silence
    // on channels 3..n and does not pay for the extra copies.
    let wanted_channels = 2usize.min(channels.outs as usize);

    // Reuse the stream map `asio-sys` keeps, because that is the same map the
    // callback reads through `driver.streams()`. Building a separate one would
    // create buffers the callback cannot see.
    let streams = driver.streams();
    let buffer_frames = {
        let mut guard = streams
            .lock()
            .map_err(|_| EngineError::Audio("ASIO stream lock poisoned".into()))?;
        let existing = guard.output.take();
        let prepared = driver
            .prepare_output_stream(existing, wanted_channels, requested_frames.map(|f| f as i32))
            .map_err(|e| EngineError::Audio(format!("ASIO buffer creation failed: {e}")))?;
        let frames = prepared
            .output
            .as_ref()
            .map(|s| s.buffer_size)
            .unwrap_or(0);
        *guard = prepared;
        frames
    };
    if buffer_frames <= 0 {
        return Err(EngineError::Audio(
            "ASIO driver reported a zero-length buffer".into(),
        ));
    }

    let format = OutputFormat {
        sample_rate,
        channels: wanted_channels,
        sample,
    };
    let frame_bytes = format.frame_bytes();

    // The mixer is sized for one driver buffer: ASIO always asks for exactly
    // `buffer_frames`, never more.
    let mut mixer = Mixer::new(buffer_frames as usize, frame_bytes);
    let big_endian = is_big_endian(&out_type);
    let shared_for_cb = Arc::clone(shared);
    let streams_for_cb = Arc::clone(&streams);

    // `queued_frames` for the latency report: ASIO has no "how much is queued"
    // query, and the honest number is the driver's own reported output latency,
    // which is what `ASIOGetLatencies` returns. It is the delay from the buffer
    // reaching the driver to the DAC, so it belongs in the same slot as WASAPI's
    // padding.
    let hardware_latency = driver.latencies().map(|l| l.output.max(0) as u32).ok();

    let callback_id = driver.add_callback(move |info| {
        let Ok(streams) = streams_for_cb.lock() else {
            return;
        };
        let Some(stream) = streams.output.as_ref() else {
            return;
        };
        let frames = stream.buffer_size as usize;
        // The return value is the count of real audio bytes; everything past it
        // is silence, which is exactly what should reach the driver for the
        // unused channels, so the count itself is not needed here.
        mixer.fill(
            &shared_for_cb,
            frames,
            frame_bytes,
            hardware_latency.unwrap_or(0),
        );

        let scratch = mixer.scratch();
        let half = (info.buffer_index as usize) & 1;
        let bytes_per_sample = frame_bytes / wanted_channels.max(1);

        // Interleaved bytes -> one plane per channel. `scratch` is already
        // silence past `filled_bytes`, so the driver gets a fully written buffer
        // on every callback, which is what stops a stale buffer from clicking.
        for ch in 0..stream.buffer_infos.len().min(wanted_channels) {
            // The struct is `packed`, so the field cannot be referenced; copy the
            // pointer out first.
            let ptr = { stream.buffer_infos[ch].buffers[half] };
            if ptr.is_null() {
                continue;
            }
            unsafe {
                match (sample, big_endian) {
                    (DeviceSample::F32, false) => {
                        let dst = ptr as *mut f32;
                        for i in 0..frames {
                            let off = (i * wanted_channels + ch) * bytes_per_sample;
                            let v = f32::from_le_bytes(
                                scratch[off..off + 4].try_into().unwrap_or([0; 4]),
                            );
                            *dst.add(i) = v;
                        }
                    }
                    (DeviceSample::F32, true) => {
                        let dst = ptr as *mut f32;
                        for i in 0..frames {
                            let off = (i * wanted_channels + ch) * bytes_per_sample;
                            let v = f32::from_le_bytes(
                                scratch[off..off + 4].try_into().unwrap_or([0; 4]),
                            );
                            *dst.add(i) = f32::from_bits(v.to_bits().swap_bytes());
                        }
                    }
                    (DeviceSample::I16, false) => {
                        let dst = ptr as *mut i16;
                        for i in 0..frames {
                            let off = (i * wanted_channels + ch) * bytes_per_sample;
                            let v = i16::from_le_bytes(
                                scratch[off..off + 2].try_into().unwrap_or([0; 2]),
                            );
                            *dst.add(i) = v;
                        }
                    }
                    (DeviceSample::I16, true) => {
                        let dst = ptr as *mut i16;
                        for i in 0..frames {
                            let off = (i * wanted_channels + ch) * bytes_per_sample;
                            let v = i16::from_le_bytes(
                                scratch[off..off + 2].try_into().unwrap_or([0; 2]),
                            );
                            *dst.add(i) = v.swap_bytes();
                        }
                    }
                    (DeviceSample::I32, false) => {
                        let dst = ptr as *mut i32;
                        for i in 0..frames {
                            let off = (i * wanted_channels + ch) * bytes_per_sample;
                            let v = i32::from_le_bytes(
                                scratch[off..off + 4].try_into().unwrap_or([0; 4]),
                            );
                            *dst.add(i) = v;
                        }
                    }
                    (DeviceSample::I32, true) => {
                        let dst = ptr as *mut i32;
                        for i in 0..frames {
                            let off = (i * wanted_channels + ch) * bytes_per_sample;
                            let v = i32::from_le_bytes(
                                scratch[off..off + 4].try_into().unwrap_or([0; 4]),
                            );
                            *dst.add(i) = v.swap_bytes();
                        }
                    }
                }
            }
        }
    });

    let info = StreamInfo {
        format,
        device_name: name,
        endpoint_frames: buffer_frames as u32,
    };
    Ok((info, driver, callback_id))
}
