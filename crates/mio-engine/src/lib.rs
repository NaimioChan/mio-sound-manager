//! Audio engine for Mio Sound Manager.
//!
//! Three independent halves:
//!
//! * [`decode`] turns a file into an immutable, device-format [`DecodedAudio`].
//!   It runs on any thread and may allocate, lock, and do IO freely. It also
//!   resamples to the device's mix format once, up front, so the render callback
//!   never has to convert anything.
//! * [`output`] owns the WASAPI stream and the dual-slot scheduling. Its render
//!   callback does nothing but copy from the active slot.
//! * [`waveform`] reduces a file to the ~1000 floats the UI draws. It decodes,
//!   because a peak envelope has to come from the samples that will be played —
//!   but it never lets those samples leave the process.
//!
//! The contract between decode and output is [`output::Slot`]: an
//! `Arc<DecodedAudio>` published by the controller, read by the audio thread.
//! Switching playback is one atomic store of the slot index, never a lock and
//! never a queue drain.

pub mod decode;
pub mod output;
pub mod waveform;

#[cfg(feature = "asio")]
pub mod asio;

mod error;

pub use decode::{DecodedAudio, Decoder, DeviceSample, OutputFormat, SourceAudio};
pub use error::{EngineError, Result};
pub use output::{
    Backend, DeviceChoice, Engine, EngineConfig, LatencyReport, PlaybackMonitor, PlaybackSnapshot,
    PublishSource,
};
pub use waveform::{DEFAULT_BUCKETS, Waveform, bucket_peaks};

