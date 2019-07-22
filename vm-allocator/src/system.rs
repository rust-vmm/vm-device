// Copyright 2018 The Chromium OS Authors. All rights reserved.
// Copyright © 2019 Intel Corporation
//
// Portions Copyright 2017 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE-BSD-3-Clause file.
//
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

use crate::address::AddressAllocator;
use crate::id::IdAllocator;
use crate::resource::ResourceAllocator;
use vm_memory::{GuestAddress, GuestUsize};

use std::collections::HashMap;
use std::result;
use std::sync::{Arc, Mutex};

/// Errors associated with system resources allocation.
#[derive(Debug)]
pub enum Error {
    /// The allocator already exists when adding a new one.
    AllocatorExist,
    /// The allocator doesn't exists when trying to allocate.
    AllocatorNotExist,
    /// Unsigned integer resource allocate fails.
    IdAllocateError(crate::resource::Error),
    /// Address resource allocate fails.
    AddressAllocateError(crate::resource::Error),
    /// Port IO address allocation fails because address is not specified.
    InvalidPortIoAddress,
}

pub type Result<T> = result::Result<T, Error>;

/// The parameters that integer resource allocation needs.
pub struct IdAllocateParameters {
    /// Resource to be allocated or freed.
    pub resource: Option<u32>,
    /// Specify resource to be allocated or freed.
    pub allocate: bool,
}

/// The parameters that address resource allocation needs.
pub struct AddrAllocateParameters {
    /// Resource to be allocated or freed.
    pub resource: Option<GuestAddress>,
    /// Size to be allocated or freed.
    pub size: GuestUsize,
    /// Specify resource to be allocated or freed.
    pub allocate: bool,
}

/// Integer resource allocation callback type.
pub type IdAllocateFunc = Box<Fn(IdAllocateParameters) -> Result<(u32)>>;

/// Address resource allocation callback type.
pub type AddrAllocateFunc = Box<Fn(AddrAllocateParameters) -> Result<(GuestAddress)>>;

/// System level allocator trait for VMM.
pub trait SystemAllocator {
    /// Integer resource allocator callback.
    fn device_id_cb(&mut self) -> Option<IdAllocateFunc> {
        None
    }

    /// Integer resource allocator callback.
    fn irq_cb(&mut self) -> Option<IdAllocateFunc> {
        None
    }

    /// Port IO address resource allocator callback.
    fn pio_addr_cb(&mut self) -> Option<AddrAllocateFunc> {
        None
    }

    /// MmiO address resource allocator callback.
    fn mmio_addr_cb(&mut self) -> Option<AddrAllocateFunc> {
        None
    }
}

/// A default system level resources allocator interface.
///
/// vm-device needs callback functions for allocating resources.
///
/// # Example
///
/// ```
/// use vm_allocator::*;
///
/// let mut vmm_allocator = DefaultSystemAllocator::new();
/// let id_cb = vmm_allocator.allocate_device_id();
///
/// ```
#[derive(Default, Clone)]
pub struct DefaultSystemAllocator {
    /// Address resource allocators mapped by name.
    pub addr_alloc: HashMap<String, Arc<Mutex<AddressAllocator>>>,
    /// Unique integer resource allocators mapped by name.
    pub id_alloc: HashMap<String, Arc<Mutex<IdAllocator>>>,
}

impl DefaultSystemAllocator {
    /// Create an empty system allocator.
    pub fn new() -> Self {
        DefaultSystemAllocator {
            addr_alloc: HashMap::new(),
            id_alloc: HashMap::new(),
        }
    }

    /// Insert port IO address resource allocator into `DefaultSystemAllocator`.
    pub fn insert_pio_addr(&mut self, allocator: Arc<Mutex<AddressAllocator>>) -> Result<()> {
        if self.addr_alloc.contains_key("pio_addr") {
            return Err(Error::AllocatorExist);
        }
        self.addr_alloc.insert("pio_addr".to_string(), allocator);
        Ok(())
    }

    /// Insert Mmio address resource allocator into `DefaultSystemAllocator`.
    pub fn insert_mmio_addr(&mut self, allocator: Arc<Mutex<AddressAllocator>>) -> Result<()> {
        if self.addr_alloc.contains_key("mmio_addr") {
            return Err(Error::AllocatorExist);
        }
        self.addr_alloc.insert("pio_addr".to_string(), allocator);
        Ok(())
    }

    /// Insert a device instance id allocator.
    ///
    /// # Arguments
    ///
    /// * `allocator`: instance id resource allocator.
    pub fn insert_device_id(&mut self, allocator: Arc<Mutex<IdAllocator>>) -> Result<()> {
        if self.id_alloc.contains_key("device_instance") {
            return Err(Error::AllocatorExist);
        }
        self.id_alloc
            .insert("device_instance".to_string(), allocator);
        Ok(())
    }

    /// Insert a IRQ number allocator.
    ///
    /// # Arguments
    ///
    /// * `allocator`: IRQ resource allocator.
    pub fn insert_irq(&mut self, allocator: Arc<Mutex<IdAllocator>>) -> Result<()> {
        if self.id_alloc.contains_key("irq") {
            return Err(Error::AllocatorExist);
        }
        self.id_alloc.insert("irq".to_string(), allocator);
        Ok(())
    }
}

impl SystemAllocator for DefaultSystemAllocator {
    fn device_id_cb(&mut self) -> Option<IdAllocateFunc> {
        let id_allocator = self.id_alloc.clone();

        let cb =
            Box::new(
                move |p: IdAllocateParameters| match id_allocator.get("device_instance") {
                    Some(allocator) => allocator
                        .lock()
                        .expect("failed to acquire lock")
                        .allocate(p.resource, 1)
                        .map_err(Error::IdAllocateError),
                    None => Err(Error::AllocatorNotExist),
                },
            ) as IdAllocateFunc;

        Some(cb)
    }

    fn irq_cb(&mut self) -> Option<IdAllocateFunc> {
        let id_allocator = self.id_alloc.clone();

        let cb = Box::new(
            move |p: IdAllocateParameters| match id_allocator.get("irq") {
                Some(allocator) => allocator
                    .lock()
                    .expect("failed to acquire lock")
                    .allocate(p.resource, 1)
                    .map_err(Error::IdAllocateError),
                None => Err(Error::AllocatorNotExist),
            },
        ) as IdAllocateFunc;

        Some(cb)
    }

    fn pio_addr_cb(&mut self) -> Option<AddrAllocateFunc> {
        let addr_allocator = self.addr_alloc.clone();

        let cb = Box::new(
            move |p: AddrAllocateParameters| match addr_allocator.get("pio_addr") {
                Some(allocator) => { match p.resource {
                    Some(addr) =>
                        allocator
                        .lock()
                        .expect("failed to acquire lock")
                        .allocate(Some(addr), p.size)
                        .map_err(Error::AddressAllocateError),
                    None => Err(Error::InvalidPortIoAddress),
                }},
                None => Err(Error::AllocatorNotExist),
            },
        ) as AddrAllocateFunc;

        Some(cb)
    }

    fn mmio_addr_cb(&mut self) -> Option<AddrAllocateFunc> {
        let addr_allocator = self.addr_alloc.clone();

        let cb = Box::new(
            move |p: AddrAllocateParameters| match addr_allocator.get("mmio_addr") {
                Some(allocator) => allocator
                    .lock()
                    .expect("failed to acquire lock")
                    .allocate(p.resource, p.size)
                    .map_err(Error::AddressAllocateError),
                None => Err(Error::AllocatorNotExist),
            },
        ) as AddrAllocateFunc;

        Some(cb)
    }
}

#[cfg(test)]
mod tests {
    use crate::id::IdAllocator;
    use crate::system::{DefaultSystemAllocator, Error, IdAllocateParameters, SystemAllocator};
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_allocate() -> Result<(), Error> {
        let mut sys = DefaultSystemAllocator::new();
        let instance_id = IdAllocator::new(1, 100).ok_or(Error::AllocatorExist)?;
        sys.insert_device_id(Arc::new(Mutex::new(instance_id)))?;

        let cb = sys.allocate_device_id().unwrap();
        let parameter = IdAllocateParameters { resource: Some(2) };

        let id = cb(parameter)?;
        assert_eq!(id, 2);
        Ok(())
    }
}
