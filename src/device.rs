// Copyright © 2019 Intel Corporation. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

//! Handles routing to devices in an address space.
use std::string::String;
use vm_memory::GuestAddress;

/// Trait for devices with basic functions.
#[allow(unused_variables)]
pub trait Device: Send {
    /// Get the device name.
    fn name(&self) -> String;
    /// Read from the guest physical address `addr` to `data`.
    fn read(&self, addr: GuestAddress, data: &mut [u8], io_type: IoType);
    /// Write `data` to the guest physical address `addr`.
    fn write(&self, addr: GuestAddress, data: &[u8], io_type: IoType);
}

/// Resource type.
#[derive(Debug, Copy, Clone)]
pub enum IoType {
    /// Port I/O resource.
    Pio,
    /// Memory I/O resource.
    Mmio,
    /// Non-exit physically backed mmap IO
    PhysicalMmio,
}
