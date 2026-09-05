//! Deterministic virtual-time Picoo pipeline simulation — REQ-PICOO-STACK-009.
//!
//! This crate is a contract harness, not a second product runtime. It drives
//! production PCP envelopes, packetization, FEC, reassembly, jitter and latest-
//! frame storage through scripted platform adapters. The small orchestration
//! model exists only to inject events that real cameras, codecs and QUIC cannot
//! reproduce deterministically in CI.

mod clock;
mod encoder;
mod harness;
mod network;
mod sim_decoder;

pub use clock::VirtualClock;
pub use encoder::{CameraFrame, EncoderCommit, EncoderFailure, SimError};
pub use harness::{PipelineCounters, SimHarness, SimSnapshot, SimTimingMode};
pub use network::{DatagramSelector, NetworkScript, SimDelivery, SimulatedNetwork};
