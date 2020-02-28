// Copyright (C) 2019-2020 Alibaba Cloud. All rights reserved.
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

use vmm_sys_util::eventfd::EventFd;

/// Reuse std::io::Result to simplify interoperability among crates.
pub type Result<T> = std::io::Result<T>;

/// Data type to store an interrupt source identifier.
pub type InterruptIndex = u32;

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
/// [InterruptSourceGroup](trait.InterruptSourceGroup.html) object, instead of abstracting each
/// interrupt source as a distinct InterruptSource.
#[allow(clippy::len_without_is_empty)]
pub trait InterruptSourceGroup {
    /// Type of configuration information for interrupt source.
    type C;

    /// Get number of interrupt sources managed by the group.
    fn len(&self) -> InterruptIndex;

    /// Get base of the assigned Interrupt Source Identifiers.
    fn base(&self) -> InterruptIndex;

    /// Enable the interrupt sources in the group to generate interrupts.
    ///
    /// The `enable()` should be invoked before invoking other methods to manipulate the
    /// InterruptSourceGroup object.
    fn enable(&self, configs: &[Self::C]) -> Result<()>;

    /// Disable the interrupt sources in the group to generate interrupts.
    fn disable(&self) -> Result<()>;

    /// Update the interrupt source group configuration.
    ///
    /// # Arguments
    /// * index: sub-index into the group.
    /// * config: configuration data for the interrupt source.
    fn update(&self, index: InterruptIndex, config: &Self::C) -> Result<()>;

    /// Returns an interrupt notifier from this interrupt.
    ///
    /// An interrupt notifier allows for external components and processes
    /// to inject interrupts into a guest, by writing to the file returned
    /// by this method.
    fn notifier(&self, _index: InterruptIndex) -> Option<&EventFd> {
        None
    }

    /// Inject an interrupt from this interrupt source into the guest.
    ///
    /// If the interrupt has an associated `interrupt_status` register, all bits set in `flag`
    /// will be atomically ORed into the `interrupt_status` register.
    fn trigger(&self, index: InterruptIndex) -> Result<()>;

    /// Mask an interrupt from this interrupt source.
    fn mask(&self, _index: InterruptIndex) -> Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::InvalidInput))
    }

    /// Unmask an interrupt from this interrupt source.
    fn unmask(&self, _index: InterruptIndex) -> Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::InvalidInput))
    }

    /// Check whether there are pending interrupts.
    fn get_pending_state(&self, _index: InterruptIndex) -> bool {
        false
    }
}
