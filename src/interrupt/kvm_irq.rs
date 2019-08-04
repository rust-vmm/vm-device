//! Manage hardware interrupts for devices based on Linux KVM framework.

use kvm_ioctls::VmFd;
use vmm_sys_util::eventfd::EventFd;

use super::*;

pub struct KvmIrqManager {
    vmfd: Arc<VmFd>,
}

impl KvmIrqManager {
    pub fn new(vmfd: Arc<VmFd>) -> Self {
        KvmIrqManager { vmfd }
    }
}

impl InterruptManager for KvmIrqManager {
    fn create_group(
        &self,
        ty: InterruptSourceType,
        base: u32,
        count: u32,
    ) -> Result<Arc<Box<dyn InterruptSourceGroup>>> {
        match ty {
            #[cfg(feature = "legacy_irq")]
            InterruptSourceType::Legacy => legacy::LegacyIrq::new(self.vmfd.clone(), base, count),
            #[cfg(feature = "generic_msi")]
            InterruptSourceType::GenericMsi => {}
            #[cfg(feature = "pci_msi")]
            InterruptSourceType::PciMsi => {}
            #[cfg(feature = "pci_msix")]
            InterruptSourceType::PciMsix => {}
        }
    }

    fn destroy_group(&self, group: Arc<Box<dyn InterruptSourceGroup>>) -> Result<()> {
        match group.get_type() {
            #[cfg(feature = "legacy_irq")]
            InterruptSourceType::Legacy => {}
        }
        Ok(())
    }
}

#[cfg(feature = "legacy_irq")]
mod legacy {
    use super::*;
    use std::os::unix::io::AsRawFd;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(super) struct LegacyIrq {
        base: u32,
        vmfd: Arc<VmFd>,
        status: AtomicUsize,
        irqfd: EventFd,
    }

    impl LegacyIrq {
        #[allow(clippy::new_ret_no_self)]
        pub fn new(
            vmfd: Arc<VmFd>,
            base: u32,
            count: u32,
        ) -> Result<Arc<Box<dyn InterruptSourceGroup>>> {
            if count != 1 {
                return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
            }
            Ok(Arc::new(Box::new(LegacyIrq {
                base,
                vmfd,
                status: AtomicUsize::new(0),
                irqfd: EventFd::new(0).unwrap(),
            })))
        }
    }

    impl InterruptSourceGroup for LegacyIrq {
        fn get_type(&self) -> InterruptSourceType {
            InterruptSourceType::Legacy
        }

        fn len(&self) -> u32 {
            1
        }

        fn get_base(&self) -> u32 {
            self.base
        }

        fn get_irqfd(&self, index: InterruptIndex) -> Option<&EventFd> {
            if index == 0 {
                Some(&self.irqfd)
            } else {
                None
            }
        }

        fn configure(&self, index: InterruptIndex, config: InterruptSourceConfig) -> Result<()> {
            if index != 0 {
                return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
            }
            if let InterruptSourceConfig::Legacy(_mask) = config {
                //self.status_mask = mask.valid_flag_mask;
                return self.vmfd.register_irqfd(self.irqfd.as_raw_fd(), self.base);
            }
            Err(std::io::Error::from_raw_os_error(libc::EINVAL))
        }

        fn trigger(&self, index: InterruptIndex, flags: u32) -> Result<()> {
            if index != 0 {
                return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
            }
            self.status.fetch_or(flags as usize, Ordering::SeqCst);
            self.irqfd.write(1)
        }

        fn ack(&self, index: InterruptIndex, flags: u32) -> Result<()> {
            if index != 0 {
                return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
            }
            self.status.fetch_and(!(flags as usize), Ordering::SeqCst);
            Ok(())
        }
    }
}

#[cfg(feature = "msi_irq")]
mod msi {
    //TODO
}

#[cfg(feature = "generic_msi")]
mod generic_msi {
    //TODO
}
