// Copyright (C) 2019 Alibaba Cloud. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Manage virtual device's interrupts based on the Linux KVM framework.
//!
//! When updaing KVM IRQ routing by ioctl(KVM_SET_GSI_ROUTING), all interrupts of the virtual
//! machine must be updated all together. The [KvmIrqRouting](struct.KvmIrqRouting.html)
//! structure is to maintain the global interrupt routing table.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kvm_bindings::{kvm_irq_routing, kvm_irq_routing_entry};
#[cfg(feature = "legacy_irq")]
use kvm_bindings::{
    KVM_IRQCHIP_IOAPIC, KVM_IRQCHIP_PIC_MASTER, KVM_IRQCHIP_PIC_SLAVE, KVM_IRQ_ROUTING_IRQCHIP,
};
use kvm_ioctls::VmFd;

use super::*;

#[cfg(feature = "legacy_irq")]
mod legacy_irq;
#[cfg(feature = "legacy_irq")]
use self::legacy_irq::LegacyIrq;

#[cfg(feature = "generic_msi")]
mod generic_msi;

#[cfg(feature = "msi_irq")]
mod msi_irq;
#[cfg(feature = "msi_irq")]
use self::msi_irq::MsiIrq;

/// Structure to manage interrupt sources for a virtual machine based on the Linux KVM framework.
///
/// The KVM framework provides methods to inject interrupts into the target virtual machines,
/// which uses irqfd to notity the KVM kernel module for injecting interrupts. When the interrupt
/// source, usually a virtual device backend in userspace, writes to the irqfd file descriptor,
/// the KVM kernel module will inject a corresponding interrupt into the target VM according to
/// the IRQ routing configuration.
pub struct KvmIrqManager {
    mgr: Mutex<KvmIrqManagerObj>,
}

impl KvmIrqManager {
    /// Create a new interrupt manager based on the Linux KVM framework.
    ///
    /// # Arguments
    /// * `vmfd`: The KVM VM file descriptor, which will be used to access the KVM subsystem.
    pub fn new(vmfd: Arc<VmFd>) -> Self {
        let vmfd2 = vmfd.clone();
        KvmIrqManager {
            mgr: Mutex::new(KvmIrqManagerObj {
                vmfd,
                groups: HashMap::new(),
                routes: Arc::new(KvmIrqRouting::new(vmfd2)),
            }),
        }
    }

    /// Prepare the interrupt manager for generating interrupts into the target VM.
    ///
    /// On x86 platforms, this will set up IRQ routings for legacy IRQs.
    pub fn initialize(&self) -> Result<()> {
        // Safe to unwrap because there's no legal way to break the mutex.
        let mgr = self.mgr.lock().unwrap();
        mgr.initialize()
    }
}

impl InterruptManager for KvmIrqManager {
    fn create_group(
        &self,
        ty: InterruptSourceType,
        base: InterruptIndex,
        count: u32,
    ) -> Result<Arc<dyn InterruptSourceGroup>> {
        // Safe to unwrap because there's no legal way to break the mutex.
        let mut mgr = self.mgr.lock().unwrap();
        mgr.create_group(ty, base, count)
    }

    fn destroy_group(&self, group: Arc<dyn InterruptSourceGroup>) -> Result<()> {
        // Safe to unwrap because there's no legal way to break the mutex.
        let mut mgr = self.mgr.lock().unwrap();
        mgr.destroy_group(group)
    }
}

struct KvmIrqManagerObj {
    vmfd: Arc<VmFd>,
    routes: Arc<KvmIrqRouting>,
    groups: HashMap<InterruptIndex, Arc<dyn InterruptSourceGroup>>,
}

impl KvmIrqManagerObj {
    fn initialize(&self) -> Result<()> {
        self.routes.initialize()?;
        Ok(())
    }

    fn create_group(
        &mut self,
        ty: InterruptSourceType,
        base: InterruptIndex,
        count: u32,
    ) -> Result<Arc<dyn InterruptSourceGroup>> {
        let group = match ty {
            #[cfg(feature = "legacy_irq")]
            InterruptSourceType::LegacyIrq => {
                LegacyIrq::new(base, count, self.vmfd.clone(), self.routes.clone())?
            }
            #[cfg(feature = "msi_irq")]
            InterruptSourceType::MsiIrq => {
                MsiIrq::new(base, count, self.vmfd.clone(), self.routes.clone())?
            }
        };

        self.groups.insert(base, group.clone());

        Ok(group)
    }

    fn destroy_group(&mut self, group: Arc<dyn InterruptSourceGroup>) -> Result<()> {
        self.groups.remove(&group.get_base());
        Ok(())
    }
}

// Use (entry.type, entry.gsi) as the hash key because entry.gsi can't uniquely identify an
// interrupt source on x86 platforms. The PIC and IOAPIC may share the same GSI on x86 platforms.
fn hash_key(entry: &kvm_irq_routing_entry) -> u64 {
    let type1 = match entry.type_ {
        KVM_IRQ_ROUTING_IRQCHIP => unsafe { entry.u.irqchip.irqchip },
        _ => 0,
    };
    (u64::from(type1) << 48 | u64::from(entry.type_) << 32) | u64::from(entry.gsi)
}

pub(super) struct KvmIrqRouting {
    vm_fd: Arc<VmFd>,
    routes: Mutex<HashMap<u64, kvm_irq_routing_entry>>,
}

impl KvmIrqRouting {
    pub(super) fn new(vm_fd: Arc<VmFd>) -> Self {
        KvmIrqRouting {
            vm_fd,
            routes: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn initialize(&self) -> Result<()> {
        // Safe to unwrap because there's no legal way to break the mutex.
        #[allow(unused_mut)]
        let mut routes = self.routes.lock().unwrap();

        #[cfg(feature = "legacy_irq")]
        self.initialize_legacy(&mut *routes)?;

        self.commit(&*routes)
    }

    fn commit(&self, routes: &HashMap<u64, kvm_irq_routing_entry>) -> Result<()> {
        // Allocate enough buffer memory.
        let elem_sz = std::mem::size_of::<kvm_irq_routing>();
        let total_sz = std::mem::size_of::<kvm_irq_routing_entry>() * routes.len() + elem_sz;
        let elem_cnt = (total_sz + elem_sz - 1) / elem_sz;
        let mut irq_routings = Vec::<kvm_irq_routing>::with_capacity(elem_cnt);
        irq_routings.resize_with(elem_cnt, Default::default);

        // Prepare the irq_routing header.
        let mut irq_routing = &mut irq_routings[0];
        irq_routing.nr = routes.len() as u32;
        irq_routing.flags = 0;

        // Safe because we have just allocated enough memory above.
        let irq_routing_entries = unsafe { irq_routing.entries.as_mut_slice(routes.len()) };
        for (idx, entry) in routes.values().enumerate() {
            irq_routing_entries[idx] = *entry;
        }

        self.vm_fd.set_gsi_routing(irq_routing)?;

        Ok(())
    }

    #[cfg(feature = "generic_msi")]
    pub(super) fn add(&self, entries: &[kvm_irq_routing_entry]) -> Result<()> {
        // Safe to unwrap because there's no legal way to break the mutex.
        let mut routes = self.routes.lock().unwrap();
        for entry in entries {
            if entry.gsi >= MAX_IRQS {
                return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
            } else if routes.contains_key(&hash_key(entry)) {
                return Err(std::io::Error::from_raw_os_error(libc::EEXIST));
            }
        }

        for entry in entries {
            let _ = routes.insert(hash_key(entry), *entry);
        }
        self.commit(&*routes)
    }

    #[cfg(feature = "generic_msi")]
    pub(super) fn remove(&self, entries: &[kvm_irq_routing_entry]) -> Result<()> {
        // Safe to unwrap because there's no legal way to break the mutex.
        let mut routes = self.routes.lock().unwrap();
        for entry in entries {
            let _ = routes.remove(&hash_key(entry));
        }
        self.commit(&routes)
    }

    #[cfg(feature = "generic_msi")]
    pub(super) fn modify(&self, entry: &kvm_irq_routing_entry) -> Result<()> {
        // Safe to unwrap because there's no legal way to break the mutex.
        let mut routes = self.routes.lock().unwrap();
        if !routes.contains_key(&hash_key(entry)) {
            return Err(std::io::Error::from_raw_os_error(libc::ENOENT));
        }

        let _ = routes.insert(hash_key(entry), *entry);
        self.commit(&routes)
    }

    #[cfg(feature = "legacy_irq")]
    fn add_legacy_entry(
        gsi: u32,
        chip: u32,
        pin: u32,
        routes: &mut HashMap<u64, kvm_irq_routing_entry>,
    ) -> Result<()> {
        let mut entry = kvm_irq_routing_entry {
            gsi,
            type_: KVM_IRQ_ROUTING_IRQCHIP,
            ..Default::default()
        };
        // Safe because we are initializing all fields of the `irqchip` struct.
        unsafe {
            entry.u.irqchip.irqchip = chip;
            entry.u.irqchip.pin = pin;
        }
        routes.insert(hash_key(&entry), entry);

        Ok(())
    }

    #[cfg(feature = "legacy_irq")]
    /// Build routings for IRQs connected to the master PIC, the slave PIC or the first IOAPIC.
    fn initialize_legacy(&self, routes: &mut HashMap<u64, kvm_irq_routing_entry>) -> Result<()> {
        // Build routings for the master PIC
        for i in 0..8 {
            if i != 2 {
                Self::add_legacy_entry(i, KVM_IRQCHIP_PIC_MASTER, i, routes)?;
            }
        }

        // Build routings for the slave PIC
        for i in 8..16 {
            Self::add_legacy_entry(i, KVM_IRQCHIP_PIC_SLAVE, i - 8, routes)?;
        }

        // Build routings for the first IOAPIC
        for i in 0..24 {
            if i == 0 {
                Self::add_legacy_entry(i, KVM_IRQCHIP_IOAPIC, 2, routes)?;
            } else if i != 2 {
                Self::add_legacy_entry(i, KVM_IRQCHIP_IOAPIC, i, routes)?;
            };
        }

        Ok(())
    }
}
