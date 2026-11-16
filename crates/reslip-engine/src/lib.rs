//! Audio engine for ReSlip.
//!
//! Two independent halves:
//!
//! * [`decode`] turns a file into an immutable, device-format [`DecodedAudio`].
//!   It runs on any thread and may allocate, lock, and do IO freely. It also
//!   resamples to the device's mix format once, up front, so the render callback
//!   never has to convert anything.
//! * [`output`] owns the WASAPI stream and the dual-slot scheduling. Its render
//!   callback does nothing but copy from the active slot.
//!
//! The contract between them is [`output::Slot`]: an `Arc<DecodedAudio>` published
//! by the controller, read by the audio thread. Switching playback is one atomic
//! store of the slot index, never a lock and never a queue drain.

pub mod decode;
pub mod output;

mod error;

pub use decode::{DecodedAudio, Decoder, DeviceSample, OutputFormat};
pub use error::{EngineError, Result};
pub use output::{Engine, EngineConfig, LatencyReport, PublishSource};
