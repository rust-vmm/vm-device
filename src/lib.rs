// Copyright © 2019 Intel Corporation. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

//! rust-vmm device model.

use std::cmp::{Ord, Ordering, PartialOrd};
use std::sync::Mutex;

pub mod device_manager;
pub mod resources;

/// IO Size.
#[derive(Debug, Copy, Clone)]
pub enum IoSize {
    /// Port I/O size.
    Pio(u16),

    /// Memory mapped I/O size.
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

/// Device IO trait adopting interior mutability pattern.
///
/// A device supporting memory based I/O should implement this trait, then
/// register itself against the different IO type ranges it handles.
/// The VMM will then dispatch IO (PIO or MMIO) VM exits by calling into the
/// registered devices read or write method from this trait.
///
/// The DeviceIo trait adopts the interior mutability pattern so we can get a
/// real concurrent multiple threads handling. For device backend drivers not
/// focusing on high performance, they may use the Mutex<T: DeviceIoMut>
/// adapter to simplify implementation.
pub trait DeviceIo: Send + Sync {
    /// Read from the guest physical address `base`, starting at `offset`.
    /// Result is placed in `data`.
    fn read(&self, base: IoAddress, offset: IoAddress, data: &mut [u8]);

    /// Write `data` to the guest physical address `base`, starting from `offset`.
    fn write(&self, base: IoAddress, offset: IoAddress, data: &[u8]);
}

/// Device IO trait without interior mutability.
///
/// Many device backend drivers will mutate itself when handling IO requests.
/// The DeviceIo trait assumes interior mutability, but it's a little complex
/// to support interior mutability. So the Mutex<T: DeviceIoMut> adapter may be
/// used to ease device backend driver implementations.
///
/// The Mutex<T: DeviceIoMut> adapter is an zero overhead abstraction without
/// performance penalty.
pub trait DeviceIoMut: Send {
    /// Read from the guest physical address `base`, starting at `offset`.
    /// Result is placed in `data`.
    fn read(&mut self, base: IoAddress, offset: IoAddress, data: &mut [u8]);

    /// Write `data` to the guest physical address `base`, starting from `offset`.
    fn write(&mut self, base: IoAddress, offset: IoAddress, data: &[u8]);
}

impl<T: DeviceIoMut> DeviceIo for Mutex<T> {
    fn read(&self, base: IoAddress, offset: IoAddress, data: &mut [u8]) {
        // Safe to unwrap() because we don't expect poisoned lock here.
        self.lock().unwrap().read(base, offset, data)
    }

    fn write(&self, base: IoAddress, offset: IoAddress, data: &[u8]) {
        // Safe to unwrap() because we don't expect poisoned lock here.
        self.lock().unwrap().write(base, offset, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Default)]
    struct MockDevice {
        data: u8,
    }

    impl DeviceIoMut for MockDevice {
        fn read(&mut self, _base: IoAddress, _offset: IoAddress, data: &mut [u8]) {
            data[0] = self.data;
        }

        fn write(&mut self, _base: IoAddress, _offset: IoAddress, data: &[u8]) {
            self.data = data[0];
        }
    }

    fn register_device(device: Arc<dyn DeviceIo>) {
        device.write(IoAddress::Mmio(0), IoAddress::Mmio(0), &[0x10u8]);
        let mut buf = [0x0u8];
        device.read(IoAddress::Mmio(0), IoAddress::Mmio(0), &mut buf);
        assert_eq!(buf[0], 0x10);
    }

    #[test]
    fn test_device_io_mut_adapter() {
        let device_mut = Arc::new(Mutex::new(MockDevice::default()));

        register_device(device_mut.clone());
        assert_eq!(device_mut.lock().unwrap().data, 0x010);
    }
}
