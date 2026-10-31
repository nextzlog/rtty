mod afsk;
pub mod baudot;
mod engine;
mod modem;
mod sender;

pub mod sim;

pub use afsk::Rtty;
pub use engine::{Burst, Command, Engine, Notice, Pick};
pub use modem::{Decode, Modem, Options, RttyOptions, Transmission, Unit};
pub use sender::Timing;

#[cfg(doctest)]
#[doc = include_str!("../../README.md")]
struct ReadmeDoctests;
