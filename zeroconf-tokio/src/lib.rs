//! `zeroconf-tokio` is a Tokio-based wrapper around the `zeroconf` crate, which provides mDNS service discovery and
//! registration capabilities.
#[macro_use]
extern crate log;

pub use zeroconf::*;

pub mod browser;
pub mod event_processor;
pub mod service;

pub use browser::MdnsBrowserAsync;
pub use service::MdnsServiceAsync;
