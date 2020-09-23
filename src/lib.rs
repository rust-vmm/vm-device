// Copyright © 2019 Intel Corporation. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

//! rust-vmm device model.

use std::cmp::{Ord, Ordering, PartialOrd};

pub mod device_manager;
pub mod interrupt;
pub mod resources;

// IO Size.
#[derive(Debug, Copy, Clone)]
enum IoSize {
    // Port I/O size.
    Pio(u16),

    // Memory mapped I/O size.
    Mmio(u64),
}

impl IoSize {
    // Get the raw value as u64 to make operation simple.
    fn raw_value(&self) -> u64 {
        match *self {
            IoSize::Pio(p) => u64::from(p),
            IoSize::Mmio(m) => m,
        }
    }
}

/// IO Addresses.
#[derive(Debug, Copy, Clone)]
pub enum IoAddress {
    /// Port I/O address.
    Pio(u16),

    /// Memory mapped I/O address.
    Mmio(u64),
}

impl IoAddress {
    // Get the raw value of IO Address to make operation simple.
    fn raw_value(&self) -> u64 {
        match *self {
            IoAddress::Pio(p) => u64::from(p),
            IoAddress::Mmio(m) => m,
        }
    }
}

impl Eq for IoAddress {}

impl PartialEq for IoAddress {
    fn eq(&self, other: &IoAddress) -> bool {
        self.raw_value() == other.raw_value()
    }
}

impl Ord for IoAddress {
    fn cmp(&self, other: &IoAddress) -> Ordering {
        self.raw_value().cmp(&other.raw_value())
    }
}

impl PartialOrd for IoAddress {
    fn partial_cmp(&self, other: &IoAddress) -> Option<Ordering> {
        self.raw_value().partial_cmp(&other.raw_value())
    }
}

/// Device IO trait.
/// A device supporting memory based I/O should implement this trait, then
/// register itself against the different IO type ranges it handles.
/// The VMM will then dispatch IO (PIO or MMIO) VM exits by calling into the
/// registered devices read or write method from this trait.
/// The DeviceIo trait adopts the interior mutability pattern
/// so we can get a real multiple threads handling.
pub trait DeviceIo: Send {
    /// Read from the guest physical address `base`, starting at `offset`.
    /// Result is placed in `data`.
    fn read(&self, base: IoAddress, offset: IoAddress, data: &mut [u8]);

    /// Write `data` to the guest physical address `base`, starting from `offset`.
    fn write(&self, base: IoAddress, offset: IoAddress, data: &[u8]);
}
