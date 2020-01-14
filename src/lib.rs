// Copyright © 2019 Intel Corporation. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

//! rust-vmm device model.

pub mod resources;

/// IO Addresses.
#[derive(Debug, Copy, Clone)]
pub enum IoAddress {
    /// Port I/O address.
    Pio(u16),

    /// Memory mapped I/O address.
    Mmio(u64),
}

/// Device IO trait.
/// A device supporting memory based I/O should implement this trait, then
/// register itself against the different IO type ranges it handles.
/// The VMM will then dispatch IO (PIO or MMIO) VM exits by calling into the
/// registered devices read or write method from this trait.
pub trait DeviceIo: Send {
    /// Read from the guest physical address `base`, starting at `offset`.
    /// Result is placed in `data`.
    fn read(&mut self, base: IoAddress, offset: IoAddress, data: &mut [u8]);

    /// Write `data` to the guest physical address `base`, starting from `offset`.
    fn write(&mut self, base: IoAddress, offset: IoAddress, data: &[u8]);
}
