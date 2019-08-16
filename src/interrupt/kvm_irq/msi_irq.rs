// Copyright (C) 2019 Alibaba Cloud. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Manage virtual device's PCI MSI/PCI MSIx interrupts based on Linux KVM framework.
//!
//! To optimize for performance by avoiding unnecessary locking and state checking, we assume that
//! the caller will take the responsibility to maintain the interrupt states and only issue valid
//! requests to this driver. If the caller doesn't obey the contract, only the current virtual
//! machine will be affected, it shouldn't break the host or other virtual machines.

use super::generic_msi::{create_msi_routing_entries, new_msi_routing_entry, MsiConfig};
use super::*;
use std::os::unix::io::AsRawFd;

pub(super) struct MsiIrq {
    base: InterruptIndex,
    count: InterruptIndex,
    vmfd: Arc<VmFd>,
    irq_routing: Arc<KvmIrqRouting>,
    msi_configs: Vec<MsiConfig>,
}

impl MsiIrq {
    #[allow(clippy::new_ret_no_self)]
    pub(super) fn new(
        base: InterruptIndex,
        count: InterruptIndex,
        vmfd: Arc<VmFd>,
        irq_routing: Arc<KvmIrqRouting>,
    ) -> Result<Arc<dyn InterruptSourceGroup>> {
        if count > MAX_MSI_IRQS_PER_DEVICE || base >= MAX_IRQS || base + count > MAX_IRQS {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }

        let mut msi_configs = Vec::with_capacity(count as usize);
        for _ in 0..count {
            msi_configs.push(MsiConfig::new());
        }

        Ok(Arc::new(MsiIrq {
            base,
            count,
            vmfd,
            irq_routing,
            msi_configs,
        }))
    }
}

impl InterruptSourceGroup for MsiIrq {
    fn get_type(&self) -> InterruptSourceType {
        InterruptSourceType::MsiIrq
    }

    fn len(&self) -> u32 {
        self.count
    }

    fn get_base(&self) -> u32 {
        self.base
    }

    fn get_irqfd(&self, index: InterruptIndex) -> Option<&EventFd> {
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
            let irqfd = self.msi_configs[i as usize].irqfd.as_raw_fd();
            self.vmfd.register_irqfd(irqfd, self.base + i)?;
        }

        Ok(())
    }

    fn disable(&self) -> Result<()> {
        // First unregister all irqfds, so it won't trigger anymore.
        for i in 0..self.count {
            let irqfd = self.msi_configs[i as usize].irqfd.as_raw_fd();
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

    fn modify(&self, index: InterruptIndex, config: &InterruptSourceConfig) -> Result<()> {
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

#[cfg(test)]
mod test {
    use super::*;
    use kvm_ioctls::{Kvm, VmFd};

    fn create_vm_fd() -> VmFd {
        let kvm = Kvm::new().unwrap();
        kvm.create_vm().unwrap()
    }

    #[test]
    fn test_msi_interrupt_group() {
        let vmfd = Arc::new(create_vm_fd());
        assert!(vmfd.create_irq_chip().is_ok());

        let rounting = Arc::new(KvmIrqRouting::new(vmfd.clone()));
        assert!(rounting.initialize().is_ok());

        let base = 168;
        let count = 32;
        let group = MsiIrq::new(base, count, vmfd.clone(), rounting.clone()).unwrap();
        let mut msi_fds = Vec::with_capacity(count as usize);

        match group.get_type() {
            InterruptSourceType::MsiIrq => {}
            _ => {
                panic!();
            }
        }

        for _ in 0..count {
            let msi_source_config = MsiIrqSourceConfig {
                high_addr: 0x1234,
                low_addr: 0x5678,
                data: 0x9876,
            };
            msi_fds.push(InterruptSourceConfig::MsiIrq(msi_source_config));
        }

        assert!(group.enable(&msi_fds).is_ok());
        assert_eq!(group.len(), count);
        assert_eq!(group.get_base(), base);

        for i in 0..count {
            let msi_source_config = MsiIrqSourceConfig {
                high_addr: i + 0x1234,
                low_addr: i + 0x5678,
                data: i + 0x9876,
            };
            assert!(group.get_irqfd(i).unwrap().write(1).is_ok());
            assert!(group.trigger(i, 0x168).is_err());
            assert!(group.trigger(i, 0).is_ok());
            assert!(group.ack(i, 0x168).is_err());
            assert!(group.ack(i, 0).is_ok());
            assert!(group
                .modify(0, &InterruptSourceConfig::MsiIrq(msi_source_config))
                .is_ok());
        }
        assert!(group.trigger(33, 0x168).is_err());
        assert!(group.ack(33, 0x168).is_err());
        assert!(group.disable().is_ok());

        assert!(MsiIrq::new(base, 33, vmfd.clone(), rounting.clone()).is_err());
        assert!(MsiIrq::new(1100, 1, vmfd.clone(), rounting.clone()).is_err());
    }
}
