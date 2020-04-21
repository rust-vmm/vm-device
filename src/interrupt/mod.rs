// Copyright (C) 2019-2020 Alibaba Cloud and Red Hat, Inc..
// All rights reserved.

// SPDX-License-Identifier: Apache-2.0

//! Traits and Structs to manage interrupt sources for devices.
//!
//! In system programming, an interrupt is a signal to the processor emitted by hardware or
//! software indicating an event that needs immediate attention. An interrupt alerts the processor
//! to a high-priority condition requiring the interruption of the current code the processor is
//! executing. The processor responds by suspending its current activities, saving its state, and
//! executing a function called an interrupt handler (or an interrupt service routine, ISR) to deal
//! with the event. This interruption is temporary, and, after the interrupt handler finishes,
//! unless handling the interrupt has emitted a fatal error, the processor resumes normal
//! activities.
//!
//! Hardware interrupts are used by devices to communicate that they require attention from the
//! operating system, or a bare-metal program running on the CPU if there are no OSes. The act of
//! initiating a hardware interrupt is referred to as an interrupt request (IRQ). Different devices
//! are usually associated with different interrupts using a unique value associated with each
//! interrupt. This makes it possible to know which hardware device caused which interrupts.
//! These interrupt values are often called IRQ lines, or just interrupt lines.
//!
//! Nowadays, IRQ lines is not the only mechanism to deliver device interrupts to processors.
//! MSI [(Message Signaled Interrupt)](https://en.wikipedia.org/wiki/Message_Signaled_Interrupts)
//! is another commonly used alternative in-band method of signaling an interrupt, using special
//! in-band messages to replace traditional out-of-band assertion of dedicated interrupt lines.
//! While more complex to implement in a device, message signaled interrupts have some significant
//! advantages over pin-based out-of-band interrupt signaling. Message signaled interrupts are
//! supported in PCI bus since its version 2.2, and in later available PCI Express bus. Some non-PCI
//! architectures also use message signaled interrupts.
//!
//! While IRQ is a term commonly used by Operating Systems when dealing with hardware
//! interrupts, the IRQ numbers managed by OSes are independent of the ones managed by VMM.
//! For simplicity sake, the term `Interrupt Source` is used instead of IRQ to represent both pin-based
//! interrupts and MSI interrupts.
//!
//! A device may support multiple types of interrupts, and each type of interrupt may support one
//! or multiple interrupt sources. For example, a PCI device may support:
//! * Legacy Irq: exactly one interrupt source.
//! * PCI MSI Irq: 1,2,4,8,16,32 interrupt sources.
//! * PCI MSIx Irq: 2^n(n=0-11) interrupt sources.
//!
//! A distinct Interrupt Source Identifier (ISID) will be assigned to each interrupt source.
//! An ID allocator will be used to allocate and free Interrupt Source Identifiers for devices.
//! To decouple the vm-device crate from the ID allocator, the vm-device crate doesn't take the
//! responsibility to allocate/free Interrupt Source IDs but only makes use of assigned IDs.

use std::fmt::{self, Display};
use std::io;
use std::ops::Deref;
use std::ops::Index;
use std::result;
use vmm_sys_util::eventfd::EventFd;

/// Errors associated with handling interrupts
#[derive(Debug)]
pub enum Error {
    /// Operation not supported for this interrupt.
    OperationNotSupported,

    /// The specified configuration is not valid.
    InvalidConfiguration,

    /// The interrupt was not enabled.
    InterruptNotEnabled,

    /// Generic IO error,
    IOError(io::Error),
}

/// Reuse std::io::Result to simplify interoperability among crates.
pub type Result<T> = std::result::Result<T, Error>;

impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Interrupt error: ")?;
        match self {
            Error::OperationNotSupported => write!(f, "operation not supported"),
            Error::InvalidConfiguration => write!(f, "invalid configuration"),
            Error::InterruptNotEnabled => write!(f, "the interrupt was not enabled"),
            Error::IOError(error) => write!(f, "{}", error),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::IOError(e)
    }
}

/// Data type to store an interrupt source identifier.
pub type InterruptIndex = u32;

/// Trait for interrupt producers.
pub trait Interrupt {
    /// Inject an interrupt from this interrupt source into the guest.
    fn trigger(&self) -> result::Result<(), io::Error>;

    /// Returns an interrupt notifier from this interrupt.
    ///
    /// An interrupt notifier allows for external components and processes
    /// to inject interrupts into a guest, by writing to the file returned
    /// by this method.
    fn notifier(&self) -> Option<&EventFd> {
        None
    }

    /// Called back when the CPU acknowledges the interrupt.
    fn acknowledge(&self) {}

    /// Returns an end-of-interrupt notifier from this interrupt.
    ///
    /// An end-of-interrupt notifier allows for external components and processes
    /// to be notified when a guest acknowledges an interrupt.  This can be used
    /// to resample and inject a level-triggered interrupt, or to mitigate the
    /// effect of lost timer interrupts.
    fn ack_notifier(&self) -> Option<&EventFd> {
        None
    }
}

/// Trait for configuring interrupts before passing them (via Box<dyn Interrupt>
/// or similar) to interrupt producers.
pub trait InterruptConfig: Interrupt {
    /// Type of configuration information for interrupt source.
    type C;

    /// Enable generation of interrupts from this interrupt source.
    fn enable(&self, config: &Self::C) -> Result<()> {
        self.update(config)
    }

    /// Disable generation of interrupts from this interrupt source.
    fn disable(&self) -> Result<()> {
        Err(Error::OperationNotSupported)
    }

    /// Update configuration of the interrupt.
    fn update(&self, _config: &Self::C) -> Result<()> {
        Err(Error::OperationNotSupported)
    }

}

pub trait StatefulInterrupt: Interrupt {
    /// For edge-triggered maskable interrupts, return whether there are pending
    /// interrupts that will be triggered on unmasking.
    ///
    /// For level-triggered interrupts, return whether the interrupt is active.
    /// An active level-triggered interrupts will be injected again when
    /// `acknowledge()` is called.
    fn is_pending(&self) -> bool;
}

pub trait MaskableInterrupt: Interrupt {
    /// Mask the interrupt.  Masked interrupts are remembered but
    /// not delivered.
    fn mask(&self) -> Result<()>;

    /// Unmask the interrupt, delivering it if it was pending.
    fn unmask(&self) -> Result<()>;
}

/// Trait to manage a group of interrupt sources for a device.
///
/// A device may support several types of interrupts, and each type of interrupt may contain one or
/// multiple continuous interrupt sources. For example, a PCI device may concurrently support:
/// * Legacy Irq: exactly one interrupt source.
/// * PCI MSI Irq: 1,2,4,8,16,32 interrupt sources.
/// * PCI MSIx Irq: 2^n(n=0-11) interrupt sources.
///
/// PCI MSI interrupts of a device may not be configured individually, and must configured as a
/// whole block. So all interrupts of the same type of a device are abstracted as an
/// [InterruptSourceGroup](struct.InterruptSourceGroup.html) object, instead of abstracting each
/// interrupt source as a distinct Interrupt.
pub struct InterruptSourceGroup<I: InterruptConfig> {
    vec: Vec<I>,
}

impl<I: InterruptConfig> InterruptSourceGroup<I> {
    /// Create a new interrupt source group from the given interrupts.
    pub fn from_interrupts(interrupts: Vec<I>) -> Self {
        Self { vec: interrupts }
    }

    /// Return whether the group manages no interrupts.
    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    /// Get number of interrupt sources managed by the group.
    pub fn len(&self) -> InterruptIndex {
        self.vec.len() as u32
    }

    /// Enable the interrupt sources in the group to generate interrupts.
    ///
    /// The `enable()` should be invoked before invoking other methods to manipulate the
    /// `InterruptSourceGroup` object.
    pub fn enable(&self, configs: &[I::C]) -> Result<()> {
        for (int, config) in self.vec.iter().zip(configs.iter()) {
            int.enable(config)?;
        }
        Ok(())
    }

    /// Disable the interrupt sources in the group to generate interrupts.
    pub fn disable(&self) -> Result<()> {
        for int in self.vec.iter() {
            int.disable()?;
        }
        Ok(())
    }

    /// Return the index-th interrupt in the group, or `None` if the index is out
    /// of bounds.
    pub fn get(&self, index: InterruptIndex) -> Option<&I> {
        self.vec.get(index as usize)
    }
}

impl<I: InterruptConfig> Index<InterruptIndex> for InterruptSourceGroup<I> {
    type Output = I;
    fn index(&self, index: u32) -> &Self::Output {
        &self.vec[index as usize]
    }
}

/// Trait to manage interrupt sources for virtual device backends.
///
/// The InterruptManager implementations should protect itself from concurrent accesses internally,
/// so it could be invoked from multi-threaded context.
pub trait InterruptManager {
    /// Interrupt type used by these sources.
    type I: InterruptConfig;

    /// Type returned by create_group().  It will usually be either a simple reference
    /// to an interrupt source group, or a reference-counted wrapper.
    type G: Deref<Target = InterruptSourceGroup<Self::I>>;

    /// Configuration used to create a group, for example a (base, count) pair
    /// or even () if no configuration is needed (such as for PCI legacy interrupts).
    type GroupConfig;

    /// Create an [InterruptSourceGroup](struct.InterruptSourceGroup.html) object to manage
    /// interrupt sources for a virtual device
    ///
    /// An [InterruptSourceGroup](struct.InterruptSourceGroup.html) object manages all interrupt
    /// sources of the same type for a virtual device.
    ///
    /// # Arguments
    /// * config: The interrupt group configuration
    fn create_group(&self, config: Self::GroupConfig) -> Result<Self::G>;
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::Cell;
    use std::rc::Rc;

    use matches::assert_matches;
    use vmm_sys_util::eventfd::{EventFd, EFD_NONBLOCK};

    struct MockInterrupt {
        enabled: Cell<bool>,
        trigger_count: Cell<u32>,
        eventfd: EventFd,
        index: InterruptIndex,
    }

    impl MockInterrupt {
        fn enabled(&self) -> bool {
            self.enabled.get()
        }
    }

    impl Interrupt for MockInterrupt {
        fn trigger(&self) -> result::Result<(), io::Error> {
            if self.enabled() {
                self.trigger_count.set(self.trigger_count.get() + 1);
            }
            Ok(())
        }

        fn notifier(&self) -> Option<&EventFd> {
            if !self.enabled() {
                None
            } else {
                Some(&self.eventfd)
            }
        }
    }

    impl InterruptConfig for MockInterrupt {
        type C = InterruptIndex;

        fn enable(&self, config: &Self::C) -> Result<()> {
            self.enabled.set(true);
            self.update(config)
        }

        fn disable(&self) -> Result<()> {
            self.enabled.set(false);
            Ok(())
        }

        fn update(&self, config: &Self::C) -> Result<()> {
            if !self.enabled() {
                Err(Error::InterruptNotEnabled)
            } else if *config != self.index {
                Err(Error::InvalidConfiguration)
            } else {
                Ok(())
            }
        }
    }

    trait EventLoop {
        fn process_eventfds(&self) -> Result<()>;
    }

    impl EventLoop for InterruptSourceGroup<MockInterrupt> {
        fn process_eventfds(&self) -> Result<()> {
            for int in self.vec.iter() {
                if let Some(ref eventfd) = int.notifier() {
                    match eventfd.read() {
                        Ok(_) => int.trigger()?,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                        Err(e) => Err(e)?,
                    }
                }
            }
            Ok(())
        }
    }

    struct MockInterruptManager;
    impl InterruptManager for MockInterruptManager {
        type I = MockInterrupt;
        type G = Rc<InterruptSourceGroup<Self::I>>;
        type GroupConfig = u32;

        fn create_group(
            &self,
            config: Self::GroupConfig,
        ) -> Result<Rc<InterruptSourceGroup<Self::I>>> {
            let ints: Vec<_> = (0..config)
                .map(|index| MockInterrupt {
                    enabled: Cell::new(false),
                    trigger_count: Cell::new(0),
                    eventfd: EventFd::new(EFD_NONBLOCK).unwrap(),
                    index,
                })
                .collect();
            Ok(Rc::new(InterruptSourceGroup::from_interrupts(ints)))
        }
    }

    #[test]
    fn create_group() {
        let mgr = MockInterruptManager;
        let grp = mgr.create_group(1).unwrap();
        assert_eq!(1, grp.len());
    }

    #[test]
    fn enable_succeeds() {
        let mgr = MockInterruptManager;
        let configs = &vec![0, 1, 2];
        let grp = mgr.create_group(3).unwrap();
        assert!(grp.enable(configs).is_ok());
    }

    #[test]
    fn enable_fails() {
        let mgr = MockInterruptManager;
        let configs = &vec![0, 1, 3];
        let grp = mgr.create_group(3).unwrap();
        assert_matches!(grp.enable(configs), Err(Error::InvalidConfiguration));
    }

    #[test]
    fn disable() {
        let mgr = MockInterruptManager;
        let configs = &vec![0];
        let grp = mgr.create_group(1).unwrap();
        assert!(grp[0].notifier().is_none());
        assert_matches!(grp[0].trigger(), Ok(()));
        assert!(grp.enable(configs).is_ok());
        assert!(grp[0].notifier().is_some());
        assert!(grp.disable().is_ok());
        assert!(grp[0].notifier().is_none());
        assert_matches!(grp[0].trigger(), Ok(()));
    }

    #[test]
    fn trigger() {
        let mgr = MockInterruptManager;
        let configs = &vec![0];
        let grp = mgr.create_group(1).unwrap();
        assert!(grp[0].trigger().is_ok());
        assert_eq!(grp[0].trigger_count.get(), 0);
        assert!(grp.enable(configs).is_ok());
        assert_eq!(grp[0].trigger_count.get(), 0);
        assert!(grp[0].trigger().is_ok());
        assert_eq!(grp[0].trigger_count.get(), 1);
        assert!(grp.disable().is_ok());
        assert_eq!(grp[0].trigger_count.get(), 1);
    }

    #[test]
    fn notifier() {
        let mgr = MockInterruptManager;
        let configs = &vec![0];
        let grp = mgr.create_group(1).unwrap();
        assert!(grp.enable(configs).is_ok());
        assert!(grp.process_eventfds().is_ok());
        assert_eq!(grp[0].trigger_count.get(), 0);
        assert!(grp[0].notifier().unwrap().write(1).is_ok());
        assert!(grp.process_eventfds().is_ok());
        assert_eq!(grp[0].trigger_count.get(), 1);
    }

    #[test]
    fn get() {
        let mgr = MockInterruptManager;
        let grp = mgr.create_group(2).unwrap();
        assert_eq!(grp.get(0).unwrap().index, 0);
        assert_eq!(grp.get(1).unwrap().index, 1);
        assert!(grp.get(2).is_none());
    }
}
