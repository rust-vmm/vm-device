// Copyright © 2019 Intel Corporation. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

//! System device management.
//!
//! [DeviceManager](struct.DeviceManager.html) responds to manage all devices
//! of virtual machine, store basic device information like name and
//! parent bus, register IO resources callback, unregister devices and help
//! VM IO exit handling.

extern crate vm_allocator;

use self::vm_allocator::{Error as AllocatorError, SystemAllocator};
use crate::device::*;
use std::cmp::{Ord, Ordering, PartialEq, PartialOrd};
use std::collections::btree_map::BTreeMap;
use std::collections::HashMap;
use std::result;
use std::sync::Arc;
use vm_memory::{Address, GuestAddress, GuestUsize};

/// Guest physical address and size pair to describe a range.
#[derive(Eq, Debug, Copy, Clone)]
pub struct Range(pub GuestAddress, pub GuestUsize);

impl PartialEq for Range {
    fn eq(&self, other: &Range) -> bool {
        self.0 == other.0
    }
}

impl Ord for Range {
    fn cmp(&self, other: &Range) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for Range {
    fn partial_cmp(&self, other: &Range) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

/// Error type for `DeviceManager` usage.
#[derive(Debug)]
pub enum Error {
    /// The insertion failed because the new device overlapped with an old device.
    Overlap,
    /// The insertion failed because device already exists.
    Exist,
    /// The removing fails because the device doesn't exist.
    NonExist,
    /// Io resource allocation failed at some index.
    IoResourceAllocate(usize, AllocatorError),
    /// IRQ allocated failed.
    IrqAllocate(AllocatorError),
    /// Instance id allocation failed.
    InstanceIdAllocate(AllocatorError),
}

/// Simplify the `Result` type.
pub type Result<T> = result::Result<T, Error>;

/// System device manager serving for all devices management and VM exit handling.
pub struct DeviceManager {
    /// System allocator reference.
    resource: SystemAllocator,
    /// Devices information mapped by instance id.
    devices: HashMap<u32, DeviceDescriptor>,
    /// Range mapping for VM exit mmio operations.
    mmio_bus: BTreeMap<Range, Arc<dyn Device>>,
    /// Range mapping for VM exit pio operations.
    pio_bus: BTreeMap<Range, Arc<dyn Device>>,
}

impl DeviceManager {
    /// Create a new `DeviceManager` with a `SystemAllocator` reference which would be
    /// used to allocate resource for devices.
    pub fn new(resource: SystemAllocator) -> Self {
        DeviceManager {
            resource,
            devices: HashMap::new(),
            mmio_bus: BTreeMap::new(),
            pio_bus: BTreeMap::new(),
        }
    }

    fn insert(&mut self, dev: DeviceDescriptor) -> Result<(u32)> {
        // Insert if the key is non-present, else report error.
        if self.devices.contains_key(&(dev.instance_id)) {
            return Err(Error::Exist);
        }
        let id = dev.instance_id;

        self.devices.insert(id, dev);
        Ok(id)
    }

    fn remove(&mut self, instance_id: u32) -> Option<DeviceDescriptor> {
        self.devices.remove(&instance_id)
    }

    fn device_descriptor(
        &self,
        id: u32,
        dev: Arc<dyn Device>,
        parent_bus: Option<Arc<dyn Device>>,
        resources: Vec<IoResource>,
        irq: Option<IrqResource>,
    ) -> DeviceDescriptor {
        DeviceDescriptor::new(id, dev.name(), dev.clone(), parent_bus, resources, irq)
    }

    // Allocate IO resources.
    // In order to transport the SystemAllocator Error, return Err with
    // the failure allocated index, or else return Ok().
    fn allocate_io_resources(&mut self, resources: &mut Vec<IoResource>) -> Result<()> {
        for (idx, res) in resources.iter_mut().enumerate() {
            match res.res_type {
                IoType::Pio => {
                    // The None PIO address resource should be a programming error.
                    let addr = res.try_unwrap();

                    res.addr = Some(
                        self.resource
                            .allocate_io_addresses(addr, res.size)
                            .map_err(|e| Error::IoResourceAllocate(idx, e))?,
                    );
                }
                IoType::PhysicalMmio | IoType::Mmio => {
                    res.addr = Some(
                        self.resource
                            .allocate_mmio_addresses(res.addr, res.size)
                            .map_err(|e| Error::IoResourceAllocate(idx, e))?,
                    );
                }
            }
        }
        Ok(())
    }

    // Free valid `resources` which means all entries have a valid address.
    fn free_io_resources(&mut self, resources: &[IoResource]) {
        for res in resources.iter() {
            // The resources addresses being free should not be None.
            let addr = res.try_unwrap();

            match res.res_type {
                IoType::Pio => self.resource.free_io_addresses(addr, res.size),
                IoType::PhysicalMmio | IoType::Mmio => {
                    self.resource.free_mmio_addresses(addr, res.size)
                }
            }
        }
    }

    // Register IO resources.
    // Return the failure registering index when fails,
    // or else return resources length.
    fn register_resources(
        &mut self,
        dev: Arc<dyn Device>,
        resources: &mut Vec<IoResource>,
    ) -> usize {
        for (idx, res) in resources.iter().enumerate() {
            // The resources addresses being registered are sucessfully allocated before.
            let addr = res.try_unwrap();

            match res.res_type {
                IoType::Pio => {
                    if self
                        .pio_bus
                        .insert(Range(addr, res.size), dev.clone())
                        .is_some()
                    {
                        return idx;
                    }
                }
                IoType::Mmio => {
                    if self
                        .mmio_bus
                        .insert(Range(addr, res.size), dev.clone())
                        .is_some()
                    {
                        return idx;
                    }
                }
                IoType::PhysicalMmio => continue,
            }
        }
        resources.len()
    }

    // Unregister resources with all entries addresses valid.
    fn unregister_resources(&mut self, resources: &[IoResource]) {
        for res in resources.iter() {
            // The resources addresses being unregistered is sucessfully allocated before.
            let addr = res.try_unwrap();

            match res.res_type {
                IoType::Pio => self.pio_bus.remove(&Range(addr, res.size)),
                IoType::Mmio => self.mmio_bus.remove(&Range(addr, res.size)),
                IoType::PhysicalMmio => continue,
            };
        }
    }

    fn allocate_irq_resource(
        &mut self,
        interrupt: Option<IrqResource>,
    ) -> Result<Option<IrqResource>> {
        match interrupt {
            Some(IrqResource(irq)) => {
                // Allocate irq resource
                let irq_num = self
                    .resource
                    .allocate_irq(irq)
                    .map_err(Error::IrqAllocate)?;
                Ok(Some(IrqResource(Some(irq_num))))
            }
            None => Ok(None),
        }
    }

    fn free_irq_resource(&mut self, interrupt: Option<IrqResource>) {
        match interrupt {
            Some(IrqResource(irq)) => self.resource.free_irq(irq),
            None => return,
        }
    }

    fn allocate_id_resource(&mut self) -> Result<u32> {
        self.resource
            .allocate_instance_id()
            .map_err(Error::InstanceIdAllocate)
    }

    fn free_id_resource(&mut self, id: u32) {
        self.resource.free_instance_id(id);
    }

    /// Register a new device with its parent bus and resources request set.
    /// Return Ok(instance_id) when sucessfully registered for caller usage.
    pub fn register_device(
        &mut self,
        dev: Arc<dyn Device>,
        parent_bus: Option<Arc<dyn Device>>,
        resources: &mut Vec<IoResource>,
        interrupt: Option<IrqResource>,
    ) -> Result<(u32)> {
        // Allocate an instance id
        let id = self.allocate_id_resource()?;

        // Reserve resources
        if let Err(Error::IoResourceAllocate(idx, e)) = self.allocate_io_resources(resources) {
            // Free allocated resources if one resource failed to allocate.
            if idx > 0 {
                self.free_io_resources(&resources[0..idx - 1]);
                self.free_id_resource(id);
                return Err(Error::IoResourceAllocate(idx, e));
            }
        }

        // Register device resources
        let register_len = self.register_resources(dev.clone(), resources);
        // Unregister and free resources once failed.
        if register_len < resources.len() && register_len > 0 {
            self.unregister_resources(&resources[0..register_len - 1]);
            self.free_io_resources(resources);
            self.free_id_resource(id);
            return Err(Error::Overlap);
        } else if register_len == 0 {
            self.free_io_resources(resources);
            self.free_id_resource(id);
            return Err(Error::Overlap);
        }

        match self.allocate_irq_resource(interrupt) {
            Ok(irq) => {
                // Set the allocated resource back
                dev.set_resources(resources, irq);

                let descriptor =
                    self.device_descriptor(id, dev, parent_bus, resources.to_vec(), irq);

                // Insert bus/device to DeviceManager with parent bus
                self.insert(descriptor)
            }
            Err(e) => {
                self.unregister_resources(resources);
                self.free_io_resources(resources);
                self.free_id_resource(id);
                Err(e)
            }
        }
    }

    /// Unregister a device from `DeviceManager`.
    pub fn unregister_device(&mut self, instance_id: u32) -> Result<()> {
        if let Some(descriptor) = self.remove(instance_id) {
            // Free instance id resource
            self.free_id_resource(instance_id);
            // Unregister resources
            self.unregister_resources(&descriptor.resources);
            // Free the resources
            self.free_io_resources(&descriptor.resources);
            self.free_irq_resource(descriptor.irq);
            Ok(())
        } else {
            Err(Error::NonExist)
        }
    }

    fn first_before(
        &self,
        addr: GuestAddress,
        io_type: IoType,
    ) -> Option<(Range, Arc<dyn Device>)> {
        match io_type {
            IoType::Pio => {
                for (range, dev) in self.pio_bus.iter().rev() {
                    if range.0 <= addr {
                        return Some((*range, dev.clone()));
                    }
                }
                None
            }
            IoType::Mmio => {
                for (range, dev) in self.mmio_bus.iter().rev() {
                    if range.0 <= addr {
                        return Some((*range, dev.clone()));
                    }
                }
                None
            }
            IoType::PhysicalMmio => None,
        }
    }

    /// Return the Device mapped the address.
    fn get_device(&self, addr: GuestAddress, io_type: IoType) -> Option<Arc<dyn Device>> {
        if let Some((Range(start, len), dev)) = self.first_before(addr, io_type) {
            if (addr.raw_value() - start.raw_value()) < len {
                return Some(dev);
            }
        }
        None
    }

    /// A helper function handling PIO/MMIO read commands during VM exit.
    ///
    /// Figure out the device according to `addr` and hand over the handling to device
    /// specific read function.
    /// Return error if failed to get the device.
    pub fn read(&self, addr: GuestAddress, data: &mut [u8], io_type: IoType) -> Result<()> {
        if let Some(dev) = self.get_device(addr, io_type) {
            dev.read(addr, data, io_type);
            Ok(())
        } else {
            Err(Error::NonExist)
        }
    }

    /// A helper function handling PIO/MMIO write commands during VM exit.
    ///
    /// Figure out the device according to `addr` and hand over the handling to device
    /// specific write function.
    /// Return error if failed to get the device.
    pub fn write(&self, addr: GuestAddress, data: &[u8], io_type: IoType) -> Result<()> {
        if let Some(dev) = self.get_device(addr, io_type) {
            dev.write(addr, data, io_type);
            Ok(())
        } else {
            Err(Error::NonExist)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::device::*;
    use crate::device_manager::*;
    use std::string::String;
    use std::sync::Mutex;

    #[test]
    fn test_dev_init() -> Result<()> {
        pub struct BusDevice {
            pub config_address: Mutex<u32>,
            pub name: String,
        }
        impl Device for BusDevice {
            /// Get the device name.
            fn name(&self) -> String {
                self.name.clone()
            }
            /// Read operation.
            fn read(&self, _addr: GuestAddress, data: &mut [u8], _io_type: IoType) {
                if data.len() > 4 {
                    for d in data {
                        *d = 0xff;
                    }
                    return;
                }
                for (idx, iter) in data.iter_mut().enumerate() {
                    let config = self.config_address.lock().expect("failed to acquire lock");
                    *iter = (*config >> (idx * 8) & 0xff) as u8;
                }
            }
            /// Write operation.
            fn write(&self, _addr: GuestAddress, data: &[u8], _io_type: IoType) {
                let mut config = self.config_address.lock().expect("failed to acquire lock");
                *config = u32::from(data[0]) & 0xff;
            }
            /// Set the allocated resource to device.
            ///
            /// This will be called by DeviceManager::register_device() to set
            /// the allocated resource from the vm_allocator back to device.
            fn set_resources(&self, _res: &[IoResource], _irq: Option<IrqResource>) {}
        }
        impl BusDevice {
            pub fn new(name: String) -> Self {
                BusDevice {
                    name,
                    config_address: Mutex::new(0x1000),
                }
            }
            pub fn get_resource(&self) -> Vec<IoResource> {
                let mut req_vec = Vec::new();
                let res = IoResource::new(Some(GuestAddress(0xcf8)), 8 as GuestUsize, IoType::Pio);

                req_vec.push(res);
                req_vec
            }
        }

        let sys_res = SystemAllocator::new(
            Some(GuestAddress(0x100)),
            Some(0x10000),
            GuestAddress(0x1000_0000),
            0x1000_0000,
            5,
            15,
            1,
        )
        .unwrap();
        let mut dev_mgr = DeviceManager::new(sys_res.clone());
        let dummy_bus = BusDevice::new("dummy-bus".to_string());
        let mut res_req = dummy_bus.get_resource();

        let id = dev_mgr.register_device(
            Arc::new(dummy_bus),
            None,
            &mut res_req,
            Some(IrqResource(None)),
        )?;
        assert_eq!(id, 1);
        Ok(())
    }
}
