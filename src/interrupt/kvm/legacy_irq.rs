// Copyright (C) 2019 Alibaba Cloud. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Manage virtual device's legacy interrupts based on Linux KVM framework.
//!
//! On x86 platforms, legacy interrupts are those managed by the Master PIC, the slave PIC and
//! IOAPICs.

use super::*;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use kvm_bindings::{KVM_IRQCHIP_IOAPIC, KVM_IRQCHIP_PIC_MASTER, KVM_IRQCHIP_PIC_SLAVE};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Maximum number of legacy interrupts supported.
pub const MAX_LEGACY_IRQS: u32 = 24;

pub(super) struct LegacyIrq {
    base: u32,
    vmfd: Arc<VmFd>,
    irqfd: EventFd,
    status: AtomicUsize,
}

impl LegacyIrq {
    #[allow(clippy::new_ret_no_self)]
    pub(super) fn new(
        base: InterruptIndex,
        count: InterruptIndex,
        vmfd: Arc<VmFd>,
        _routes: Arc<KvmIrqRouting>,
    ) -> Result<Self> {
        if count != 1 {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }

        if base >= MAX_LEGACY_IRQS {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }

        Ok(LegacyIrq {
            base,
            vmfd,
            irqfd: EventFd::new(0)?,
            status: AtomicUsize::new(0),
        })
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
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

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    /// Build routings for IRQs connected to the master PIC, the slave PIC or the first IOAPIC.
    pub(super) fn initialize_legacy(
        routes: &mut HashMap<u64, kvm_irq_routing_entry>,
    ) -> Result<()> {
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
        for i in 0..MAX_LEGACY_IRQS {
            if i == 0 {
                Self::add_legacy_entry(i, KVM_IRQCHIP_IOAPIC, 2, routes)?;
            } else if i != 2 {
                Self::add_legacy_entry(i, KVM_IRQCHIP_IOAPIC, i, routes)?;
            };
        }

        Ok(())
    }

    #[cfg(any(target_arch = "aarch", target_arch = "aarch64"))]
    pub(super) fn initialize_legacy(
        _routes: &mut HashMap<u64, kvm_irq_routing_entry>,
    ) -> Result<()> {
        //TODO
        Ok(())
    }
}

impl InterruptSourceGroup for LegacyIrq {
    fn interrupt_type(&self) -> InterruptSourceType {
        InterruptSourceType::LegacyIrq
    }

    fn len(&self) -> u32 {
        1
    }

    fn base(&self) -> u32 {
        self.base
    }

    fn irqfd(&self, index: InterruptIndex) -> Option<&EventFd> {
        if index != 0 {
            None
        } else {
            Some(&self.irqfd)
        }
    }

    fn flags(&self, index: InterruptIndex) -> u32 {
        if index == 0 {
            self.status.load(Ordering::SeqCst) as u32
        } else {
            0
        }
    }

    fn enable(&self, configs: &[InterruptSourceConfig]) -> Result<()> {
        if configs.len() != 1 {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }
        // The IRQ routings for legacy IRQs have been configured during
        // KvmIrqManager::initialize(), so only need to register irqfd to the KVM driver.
        self.vmfd.register_irqfd(&self.irqfd, self.base)
    }

    fn disable(&self) -> Result<()> {
        self.vmfd.unregister_irqfd(&self.irqfd, self.base)
    }

    fn update(&self, index: InterruptIndex, _config: &InterruptSourceConfig) -> Result<()> {
        // For legacy interrupts, the routing configuration is managed by the PIC/IOAPIC interrupt
        // controller drivers, so nothing to do here.
        if index != 0 {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }
        Ok(())
    }

    fn trigger(&self, index: InterruptIndex, flags: u32) -> Result<()> {
        if index != 0 {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }
        // Set interrupt status bits before writing to the irqfd.
        self.status.fetch_or(flags as usize, Ordering::SeqCst);
        self.irqfd.write(1)
    }

    fn ack(&self, index: InterruptIndex, flags: u32) -> Result<()> {
        if index != 0 {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }
        // Clear interrupt status bits.
        self.status.fetch_and(!(flags as usize), Ordering::SeqCst);
        Ok(())
    }
}
