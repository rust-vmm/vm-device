// Copyright © 2019 Intel Corporation. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

//! rust-vmm device model.

extern crate vm_memory;

use vm_memory::GuestAddress;

pub mod resources;

/// IO Addresses.
#[derive(Debug, Copy, Clone)]
pub enum IoAddress {
    /// Port I/O address.
    Pio(u16),

    /// Memory mapped I/O address.
    Mmio(GuestAddress),
}

/// Device IO trait.
/// A device supporting memory based I/O should implement this trait. For
/// device that has one or several IO (PIO or MMIO) address space, it
/// registers itself against the different IO type ranges it handles with
/// a unique token to distinguish different windows. The VMM will then
/// dispatch IO (PIO or MMIO) VM exits by calling into the registered devices
/// read or write method from this trait.
pub trait DeviceIo: Send {
    /// Read from `offset` of IO address space specified by `token` to `data`.
    fn read(&mut self, offset: IoAddress, token: usize, data: &mut [u8]);

    /// Write `data` to `offset` of IO address space specified by `token`.
    fn write(&mut self, offset: IoAddress, token: usize, data: &[u8]);
}
