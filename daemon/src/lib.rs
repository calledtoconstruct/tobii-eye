//! Userspace daemon for the Tobii Eye Tracker 5.
//!
//! The process opens the tracker with libusb and serves the existing
//! tobiifreed socket and WebSocket framing. This is a translation of the
//! protocol in Aetherall/tobiifree and is GPL-3.0-only.

pub mod config;
pub mod frame;
pub mod gaze;
pub mod ipc;
pub mod parser;
pub mod serve;
pub mod session;
pub mod tlv;
pub mod usb;

pub use gaze::GazeSample;
pub use session::{Action, Session};
