// Copyright (C) 2019 Alibaba Cloud. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Manage virtual device's PCI MSI/PCI MSIx interrupts based on Linux KVM framework.
//!
//! To optimize for performance by avoiding unnecessary locking and state checking, we assume that
//! the caller will take the responsibility to maintain the interrupt states and only issue valid
//! requests to this driver. If the caller doesn't obey the contract, only the current virtual
//! machine will be affected, it shouldn't break the host or other virtual machines.

use super::msi_irq::{create_msi_routing_entries, new_msi_routing_entry, MsiConfig};
use super::*;

pub(super) struct PciMsiIrq {
    base: InterruptIndex,
    count: InterruptIndex,
    vmfd: Arc<VmFd>,
    irq_routing: Arc<KvmIrqRouting>,
    msi_configs: Vec<MsiConfig>,
}

impl PciMsiIrq {
    #[allow(clippy::new_ret_no_self)]
    pub(super) fn new(
        base: InterruptIndex,
        count: InterruptIndex,
        max_msi_irqs: InterruptIndex,
        vmfd: Arc<VmFd>,
        irq_routing: Arc<KvmIrqRouting>,
    ) -> Result<Self> {
        if count > max_msi_irqs || base >= MAX_IRQS || base + count > MAX_IRQS {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }

        let mut msi_configs = Vec::with_capacity(count as usize);
        for _ in 0..count {
            msi_configs.push(MsiConfig::new());
        }

        Ok(PciMsiIrq {
            base,
            count,
            vmfd,
            irq_routing,
            msi_configs,
        })
    }
}

impl InterruptSourceGroup for PciMsiIrq {
    fn interrupt_type(&self) -> InterruptSourceType {
        InterruptSourceType::PciMsiIrq
    }

    fn len(&self) -> u32 {
        self.count
    }

    fn base(&self) -> u32 {
        self.base
    }

    fn irqfd(&self, index: InterruptIndex) -> Option<&EventFd> {
        if index >= self.count {
            None
        } else {
            let msi_config = &self.msi_configs[index as usize];
            Some(&msi_config.irqfd)
        }
    }

    fn enable(&self, configs: &[InterruptSourceConfig]) -> Result<()> {
        if configs.len() != self.count as usize {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }

        // First add IRQ routings for all the MSI interrupts.
        let entries = create_msi_routing_entries(self.base, configs)?;
        self.irq_routing.add(&entries)?;

        // Then register irqfds to the KVM module.
        for i in 0..self.count {
            let irqfd = &self.msi_configs[i as usize].irqfd;
            self.vmfd.register_irqfd(irqfd, self.base + i)?;
        }

        Ok(())
    }

    fn disable(&self) -> Result<()> {
        // First unregister all irqfds, so it won't trigger anymore.
        for i in 0..self.count {
            let irqfd = &self.msi_configs[i as usize].irqfd;
            self.vmfd.unregister_irqfd(irqfd, self.base + i)?;
        }

        // Then tear down the IRQ routings for all the MSI interrupts.
        let mut entries = Vec::with_capacity(self.count as usize);
        for i in 0..self.count {
            // Safe to unwrap because there's no legal way to break the mutex.
            let msicfg = self.msi_configs[i as usize].config.lock().unwrap();
            let entry = new_msi_routing_entry(self.base + i, &*msicfg);
            entries.push(entry);
        }
        self.irq_routing.remove(&entries)?;

        Ok(())
    }

    #[allow(irrefutable_let_patterns)]
    fn update(&self, index: InterruptIndex, config: &InterruptSourceConfig) -> Result<()> {
        if index >= self.count {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }

        if let InterruptSourceConfig::MsiIrq(ref cfg) = config {
            // Safe to unwrap because there's no legal way to break the mutex.
            let entry = {
                let mut msicfg = self.msi_configs[index as usize].config.lock().unwrap();
                msicfg.high_addr = cfg.high_addr;
                msicfg.low_addr = cfg.low_addr;
                msicfg.data = cfg.data;
                new_msi_routing_entry(self.base + index, &*msicfg)
            };
            self.irq_routing.modify(&entry)
        } else {
            Err(std::io::Error::from_raw_os_error(libc::EINVAL))
        }
    }

    fn trigger(&self, index: InterruptIndex, flags: u32) -> Result<()> {
        // Assume that the caller will maintain the interrupt states and only call this function
        // when suitable.
        if index >= self.count || flags != 0 {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }
        let msi_config = &self.msi_configs[index as usize];
        msi_config.irqfd.write(1)
    }

    fn ack(&self, index: InterruptIndex, flags: u32) -> Result<()> {
        // It's a noop to acknowledge an edge triggered MSI interrupts.
        if index >= self.count || flags != 0 {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }
        Ok(())
    }
}
