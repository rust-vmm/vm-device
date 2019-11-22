// Copyright © 2019 Intel Corporation. All Rights Reserved.
// SPDX-License-Identifier: (Apache-2.0 OR BSD-3-Clause)

//! System level device management.
//!
//! [IoManager](struct.IoManager.html) is respondsible for managing
//! all devices of virtual machine, registering IO resources callback,
//! unregistering devices and helping VM IO exit handling.
//！
//！VMM would be responsible for getting device resource request, ask
//! vm_allocator to allocate the resources, ask vm_device to register the
//! devices IO ranges, and finally set resources to virtual device.

use crate::resources::Resource;
use crate::{DeviceIo, IoAddress, IoSize};

use std::cmp::{Ord, Ordering, PartialEq, PartialOrd};
use std::collections::btree_map::BTreeMap;
use std::result;
use std::sync::Arc;

/// Error type for `IoManager` usage.
#[derive(Debug)]
pub enum Error {
    /// The inserting device overlaps with a current device.
    DeviceOverlap,
    /// The device doesn't exist.
    NoDevice,
}

/// Simplify the `Result` type.
pub type Result<T> = result::Result<T, Error>;

// Structure describing an IO range.
#[derive(Debug, Copy, Clone)]
struct IoRange {
    base: IoAddress,
    size: IoSize,
}

impl IoRange {
    fn new_pio_range(base: u16, size: u16) -> Self {
        IoRange {
            base: IoAddress::Pio(base),
            size: IoSize::Pio(size),
        }
    }
    fn new_mmio_range(base: u64, size: u64) -> Self {
        IoRange {
            base: IoAddress::Mmio(base),
            size: IoSize::Mmio(size),
        }
    }
}

impl Eq for IoRange {}

impl PartialEq for IoRange {
    fn eq(&self, other: &IoRange) -> bool {
        self.base == other.base
    }
}

impl Ord for IoRange {
    fn cmp(&self, other: &IoRange) -> Ordering {
        self.base.cmp(&other.base)
    }
}

impl PartialOrd for IoRange {
    fn partial_cmp(&self, other: &IoRange) -> Option<Ordering> {
        self.base.partial_cmp(&other.base)
    }
}

/// System IO manager serving for all devices management and VM exit handling.
#[derive(Default)]
pub struct IoManager {
    /// Range mapping for VM exit pio operations.
    pio_bus: BTreeMap<IoRange, Arc<dyn DeviceIo>>,
    /// Range mapping for VM exit mmio operations.
    mmio_bus: BTreeMap<IoRange, Arc<dyn DeviceIo>>,
}

impl IoManager {
    /// Create an default IoManager with empty IO member.
    pub fn new() -> Self {
        IoManager::default()
    }
    /// Register a new device IO with its allocated resources.
    /// VMM is responsible for providing the allocated resources to virtual device.
    ///
    /// # Arguments
    ///
    /// * `device`: device instance object to be registered
    /// * `resources`: resources that this device owns, might include
    ///                port I/O and memory-mapped I/O ranges, irq number, etc.
    pub fn register_device_io(
        &mut self,
        device: Arc<dyn DeviceIo>,
        resources: &[Resource],
    ) -> Result<()> {
        // Register and mark device resources
        // The resources addresses being registered are sucessfully allocated before.
        for (idx, res) in resources.iter().enumerate() {
            match *res {
                Resource::PioAddressRange { base, size } => {
                    if self
                        .pio_bus
                        .insert(IoRange::new_pio_range(base, size), device.clone())
                        .is_some()
                    {
                        // Unregister registered resources.
                        self.unregister_device_io(&resources[0..idx])
                            .expect("failed to unregister devices");

                        return Err(Error::DeviceOverlap);
                    }
                }
                Resource::MmioAddressRange { base, size } => {
                    if self
                        .mmio_bus
                        .insert(IoRange::new_mmio_range(base, size), device.clone())
                        .is_some()
                    {
                        // Unregister registered resources.
                        self.unregister_device_io(&resources[0..idx])
                            .expect("failed to unregister devices");

                        return Err(Error::DeviceOverlap);
                    }
                }
                _ => continue,
            }
        }
        Ok(())
    }

    /// Unregister a device from `IoManager`, e.g. users specified removing.
    /// VMM pre-fetches the resources e.g. dev.get_assigned_resources()
    /// VMM is responsible for freeing the resources.
    ///
    /// # Arguments
    ///
    /// * `resources`: resources that this device owns, might include
    ///                port I/O and memory-mapped I/O ranges, irq number, etc.
    pub fn unregister_device_io(&mut self, resources: &[Resource]) -> Result<()> {
        for res in resources.iter() {
            match *res {
                Resource::PioAddressRange { base, size } => {
                    self.pio_bus.remove(&IoRange::new_pio_range(base, size));
                }
                Resource::MmioAddressRange { base, size } => {
                    self.mmio_bus.remove(&IoRange::new_mmio_range(base, size));
                }
                _ => continue,
            }
        }
        Ok(())
    }

    fn get_entry(&self, addr: IoAddress) -> Option<(&IoRange, &Arc<dyn DeviceIo>)> {
        match addr {
            IoAddress::Pio(a) => self
                .pio_bus
                .range(..=&IoRange::new_pio_range(a, 0))
                .nth_back(0),
            IoAddress::Mmio(a) => self
                .mmio_bus
                .range(..=&IoRange::new_mmio_range(a, 0))
                .nth_back(0),
        }
    }

    // Return the Device mapped `addr` and the base address.
    fn get_device(&self, addr: IoAddress) -> Option<(&Arc<dyn DeviceIo>, IoAddress)> {
        if let Some((range, dev)) = self.get_entry(addr) {
            if (addr.raw_value() - range.base.raw_value()) < range.size.raw_value() {
                return Some((dev, range.base));
            }
        }
        None
    }

    /// A helper function handling PIO read command during VM exit.
    /// The virtual device itself provides mutable ability and thead-safe protection.
    ///
    /// Return error if failed to get the device.
    pub fn pio_read(&self, addr: u16, data: &mut [u8]) -> Result<()> {
        if let Some((device, base)) = self.get_device(IoAddress::Pio(addr)) {
            device.read(base, IoAddress::Pio(addr - (base.raw_value() as u16)), data);
            Ok(())
        } else {
            Err(Error::NoDevice)
        }
    }

    /// A helper function handling PIO write command during VM exit.
    /// The virtual device itself provides mutable ability and thead-safe protection.
    ///
    /// Return error if failed to get the device.
    pub fn pio_write(&self, addr: u16, data: &[u8]) -> Result<()> {
        if let Some((device, base)) = self.get_device(IoAddress::Pio(addr)) {
            device.write(base, IoAddress::Pio(addr - (base.raw_value() as u16)), data);
            Ok(())
        } else {
            Err(Error::NoDevice)
        }
    }

    /// A helper function handling MMIO read command during VM exit.
    /// The virtual device itself provides mutable ability and thead-safe protection.
    ///
    /// Return error if failed to get the device.
    pub fn mmio_read(&self, addr: u64, data: &mut [u8]) -> Result<()> {
        if let Some((device, base)) = self.get_device(IoAddress::Mmio(addr)) {
            device.read(base, IoAddress::Mmio(addr - base.raw_value()), data);
            Ok(())
        } else {
            Err(Error::NoDevice)
        }
    }

    /// A helper function handling MMIO write command during VM exit.
    /// The virtual device itself provides mutable ability and thead-safe protection.
    ///
    /// Return error if failed to get the device.
    pub fn mmio_write(&self, addr: u64, data: &[u8]) -> Result<()> {
        if let Some((device, base)) = self.get_device(IoAddress::Mmio(addr)) {
            device.write(base, IoAddress::Mmio(addr - base.raw_value()), data);
            Ok(())
        } else {
            Err(Error::NoDevice)
        }
    }
}
