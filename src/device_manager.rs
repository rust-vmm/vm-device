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
use crate::DeviceIo;

use std::collections::btree_map::BTreeMap;
use std::result;
use std::sync::Arc;

/// Error type for `IoManager` usage.
#[derive(Debug)]
pub enum Error {
    /// The inserting device overlaps with a current device.
    DeviceOverlap,
}

/// Simplify the `Result` type.
pub type Result<T> = result::Result<T, Error>;

/// System IO manager serving for all devices management and VM exit handling.
#[derive(Default)]
pub struct IoManager {
    /// Range mapping for VM exit pio operations.
    pio_bus: BTreeMap<(u16, u16), Arc<dyn DeviceIo>>,
    /// Range mapping for VM exit mmio operations.
    mmio_bus: BTreeMap<(u64, u64), Arc<dyn DeviceIo>>,
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
                    if self.pio_bus.insert((base, size), device.clone()).is_some() {
                        // Unregister registered resources.
                        self.unregister_device_io(&resources[0..idx])
                            .expect("failed to unregister devices");

                        return Err(Error::DeviceOverlap);
                    }
                }
                Resource::MmioAddressRange { base, size } => {
                    if self.mmio_bus.insert((base, size), device.clone()).is_some() {
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
                    self.pio_bus.remove(&(base, size));
                }
                Resource::MmioAddressRange { base, size } => {
                    self.mmio_bus.remove(&(base, size));
                }
                _ => continue,
            }
        }
        Ok(())
    }
}
