mod afsk;
pub mod baudot;
mod crc;
mod engine;
mod ft;
mod ldpc;
mod message;
mod modem;
mod proto;
mod resample;
mod tables;
#[cfg(test)]
mod vectors;

pub mod sim;

pub use afsk::Rtty;
pub use ft::{Ft4, Ft8};
pub use message::{fields, pack, unpack, Fields};
pub use modem::{
    Decode, FtOptions, Modem, Options, Parity, RttyOptions, Transmission, Unit, MAX_HZ,
};

#[cfg(doctest)]
#[doc = include_str!("../../README.md")]
struct ReadmeDoctests;
