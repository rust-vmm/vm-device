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
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use crate::PioAddress;
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

/// Structure describing an IO range.
#[derive(Debug, Copy, Clone, Eq)]
pub struct IoRange {
    base: IoAddress,
    size: IoSize,
}

impl IoRange {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    fn new_pio_range(base: u16, size: u16) -> Self {
        IoRange {
            base: IoAddress(base as u64),
            size: IoSize(size as u64),
        }
    }

    fn new_mmio_range(base: u64, size: u64) -> Self {
        IoRange {
            base: IoAddress(base),
            size: IoSize(size),
        }
    }
}

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
#[derive(Clone, Default)]
pub struct IoManager {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
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
                #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
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
                #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
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

    /// A helper function handling MMIO read command during VM exit.
    /// The virtual device itself provides mutable ability and thead-safe protection.
    ///
    /// Return error if failed to get the device.
    pub fn mmio_read(&self, addr: u64, data: &mut [u8]) -> Result<()> {
        self.get_device(IoAddress(addr))
            .map(|(device, base)| device.read(base, IoAddress(addr - base.raw_value()), data))
            .ok_or(Error::NoDevice)
    }

    /// A helper function handling MMIO write command during VM exit.
    /// The virtual device itself provides mutable ability and thead-safe protection.
    ///
    /// Return error if failed to get the device.
    pub fn mmio_write(&self, addr: u64, data: &[u8]) -> Result<()> {
        self.get_device(IoAddress(addr))
            .map(|(device, base)| device.write(base, IoAddress(addr - base.raw_value()), data))
            .ok_or(Error::NoDevice)
    }

    // Return the Device mapped `addr` and the base address.
    fn get_device(&self, addr: IoAddress) -> Option<(&Arc<dyn DeviceIo>, IoAddress)> {
        let range = IoRange::new_mmio_range(addr.raw_value(), 0);
        if let Some((range, dev)) = self.mmio_bus.range(..=&range).nth_back(0) {
            if (addr.raw_value() - range.base.raw_value()) < range.size.raw_value() {
                return Some((dev, range.base));
            }
        }
        None
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
impl IoManager {
    /// A helper function handling PIO read command during VM exit.
    /// The virtual device itself provides mutable ability and thead-safe protection.
    ///
    /// Return error if failed to get the device.
    pub fn pio_read(&self, addr: u16, data: &mut [u8]) -> Result<()> {
        self.get_pio_device(PioAddress(addr))
            .map(|(device, base)| device.pio_read(base, PioAddress(addr - base.raw_value()), data))
            .ok_or(Error::NoDevice)
    }

    /// A helper function handling PIO write command during VM exit.
    /// The virtual device itself provides mutable ability and thead-safe protection.
    ///
    /// Return error if failed to get the device.
    pub fn pio_write(&self, addr: u16, data: &[u8]) -> Result<()> {
        self.get_pio_device(PioAddress(addr))
            .map(|(device, base)| device.pio_write(base, PioAddress(addr - base.raw_value()), data))
            .ok_or(Error::NoDevice)
    }

    // Return the Device mapped `addr` and the base address.
    fn get_pio_device(&self, addr: PioAddress) -> Option<(&Arc<dyn DeviceIo>, PioAddress)> {
        let range = IoRange::new_pio_range(addr.raw_value(), 0);
        if let Some((range, dev)) = self.pio_bus.range(..=&range).nth_back(0) {
            if (addr.raw_value() as u64 - range.base.raw_value()) < range.size.raw_value() {
                return Some((dev, PioAddress(range.base.0 as u16)));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    const PIO_ADDRESS_SIZE: u16 = 4;
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    const PIO_ADDRESS_BASE: u16 = 0x40;
    const MMIO_ADDRESS_SIZE: u64 = 0x8765_4321;
    const MMIO_ADDRESS_BASE: u64 = 0x1234_5678;
    const LEGACY_IRQ: u32 = 4;
    const CONFIG_DATA: u32 = 0x1234;

    struct DummyDevice {
        config: Mutex<u32>,
    }

    impl DummyDevice {
        fn new(config: u32) -> Self {
            DummyDevice {
                config: Mutex::new(config),
            }
        }
    }

    impl DeviceIo for DummyDevice {
        fn read(&self, _base: IoAddress, _offset: IoAddress, data: &mut [u8]) {
            if data.len() > 4 {
                return;
            }
            for (idx, iter) in data.iter_mut().enumerate() {
                let config = self.config.lock().expect("failed to acquire lock");
                *iter = (*config >> (idx * 8) & 0xff) as u8;
            }
        }

        fn write(&self, _base: IoAddress, _offset: IoAddress, data: &[u8]) {
            let mut config = self.config.lock().expect("failed to acquire lock");
            *config = u32::from(data[0]) & 0xff;
        }

        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        fn pio_read(&self, _base: PioAddress, _offset: PioAddress, data: &mut [u8]) {
            if data.len() > 4 {
                return;
            }
            for (idx, iter) in data.iter_mut().enumerate() {
                let config = self.config.lock().expect("failed to acquire lock");
                *iter = (*config >> (idx * 8) & 0xff) as u8;
            }
        }

        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        fn pio_write(&self, _base: PioAddress, _offset: PioAddress, data: &[u8]) {
            let mut config = self.config.lock().expect("failed to acquire lock");
            *config = u32::from(data[0]) & 0xff;
        }
    }

    #[test]
    fn test_clone_io_manager() {
        let mut io_mgr = IoManager::new();
        let dummy = DummyDevice::new(0);
        let dum = Arc::new(dummy);

        let mut resource: Vec<Resource> = Vec::new();
        let mmio = Resource::MmioAddressRange {
            base: MMIO_ADDRESS_BASE,
            size: MMIO_ADDRESS_SIZE,
        };
        let irq = Resource::LegacyIrq(LEGACY_IRQ);

        resource.push(mmio);
        resource.push(irq);

        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        {
            let pio = Resource::PioAddressRange {
                base: PIO_ADDRESS_BASE,
                size: PIO_ADDRESS_SIZE,
            };
            resource.push(pio);
        }

        assert!(io_mgr.register_device_io(dum.clone(), &resource).is_ok());

        let io_mgr2 = io_mgr.clone();
        assert_eq!(io_mgr2.mmio_bus.len(), 1);

        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        {
            assert_eq!(io_mgr2.pio_bus.len(), 1);

            let (dev, addr) = io_mgr2
                .get_device(IoAddress(MMIO_ADDRESS_BASE + 1))
                .unwrap();
            assert_eq!(Arc::strong_count(dev), 5);

            assert_eq!(addr, IoAddress(MMIO_ADDRESS_BASE));

            drop(io_mgr);
            assert_eq!(Arc::strong_count(dev), 3);

            drop(io_mgr2);
            assert_eq!(Arc::strong_count(&dum), 1);
        }
    }

    #[test]
    fn test_register_unregister_device_io() {
        let mut io_mgr = IoManager::new();
        let dummy = DummyDevice::new(0);
        let dum = Arc::new(dummy);

        let mut resource: Vec<Resource> = Vec::new();
        let mmio = Resource::MmioAddressRange {
            base: MMIO_ADDRESS_BASE,
            size: MMIO_ADDRESS_SIZE,
        };
        let irq = Resource::LegacyIrq(LEGACY_IRQ);

        resource.push(mmio);
        resource.push(irq);

        assert!(io_mgr.register_device_io(dum.clone(), &resource).is_ok());
        assert!(io_mgr.unregister_device_io(&resource).is_ok())
    }

    #[test]
    fn test_mmio_read_write() {
        let mut io_mgr: IoManager = Default::default();
        let dum = Arc::new(DummyDevice::new(CONFIG_DATA));
        let mut resource: Vec<Resource> = Vec::new();

        let mmio = Resource::MmioAddressRange {
            base: MMIO_ADDRESS_BASE,
            size: MMIO_ADDRESS_SIZE,
        };
        resource.push(mmio);
        assert!(io_mgr.register_device_io(dum.clone(), &resource).is_ok());

        let mut data = [0; 4];
        assert!(io_mgr.mmio_read(MMIO_ADDRESS_BASE, &mut data).is_ok());
        assert_eq!(data, [0x34, 0x12, 0, 0]);

        assert!(io_mgr
            .mmio_read(MMIO_ADDRESS_BASE + MMIO_ADDRESS_SIZE, &mut data)
            .is_err());

        data = [0; 4];
        assert!(io_mgr.mmio_write(MMIO_ADDRESS_BASE, &data).is_ok());
        assert_eq!(*dum.config.lock().unwrap(), 0);

        assert!(io_mgr
            .mmio_write(MMIO_ADDRESS_BASE + MMIO_ADDRESS_SIZE, &data)
            .is_err());
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    #[test]
    fn test_pio_read_write() {
        let mut io_mgr: IoManager = Default::default();
        let dum = Arc::new(DummyDevice::new(CONFIG_DATA));
        let mut resource: Vec<Resource> = Vec::new();

        let pio = Resource::PioAddressRange {
            base: PIO_ADDRESS_BASE,
            size: PIO_ADDRESS_SIZE,
        };
        resource.push(pio);
        assert!(io_mgr.register_device_io(dum.clone(), &resource).is_ok());

        let mut data = [0; 4];
        assert!(io_mgr.pio_read(PIO_ADDRESS_BASE, &mut data).is_ok());
        assert_eq!(data, [0x34, 0x12, 0, 0]);

        assert!(io_mgr
            .pio_read(PIO_ADDRESS_BASE + PIO_ADDRESS_SIZE, &mut data)
            .is_err());

        data = [0; 4];
        assert!(io_mgr.pio_write(PIO_ADDRESS_BASE, &data).is_ok());
        assert_eq!(*dum.config.lock().unwrap(), 0);

        assert!(io_mgr
            .pio_write(PIO_ADDRESS_BASE + PIO_ADDRESS_SIZE, &data)
            .is_err());
    }

    #[test]
    fn test_device_manager_data_structs() {
        let range1 = IoRange::new_mmio_range(0x1000, 0x1000);
        let range2 = IoRange::new_mmio_range(0x1000, 0x2000);
        let range3 = IoRange::new_mmio_range(0x2000, 0x1000);

        assert_eq!(range1, range1.clone());
        assert_eq!(range1, range2);
        assert!(range1 < range3);
    }
}
