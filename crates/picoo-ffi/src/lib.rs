//! C ABI entry points for mobile platforms — REQ-PICOO-STACK-003.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

pub mod c_diagnostics;
pub mod c_discovery;
pub mod c_media;
pub mod c_pairing;
pub mod c_sender;
pub mod c_sender_control;
mod handles;

pub use c_sender::*;
pub use c_sender_control::*;
pub use c_media::*;
pub use c_pairing::*;
pub use c_discovery::*;
pub use c_diagnostics::*;
pub use handles::PicooSessionHandle;

#[cfg(any(target_os = "android", test))]
mod android_jni;

#[cfg(test)]
mod tests;
