// Copyright (C) 2019-2020 Alibaba Cloud, Red Hat, Inc and Amazon.com, Inc. or its affiliates.
// All Rights Reserved.

// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

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

pub mod legacy;
pub mod msi;

use std::fmt::{self, Display};
use std::ops::Deref;
use std::sync::Arc;

/// Errors associated with handling interrupts
#[derive(Debug)]
pub enum Error {
    /// Operation not supported for this interrupt.
    OperationNotSupported,

    /// The specified configuration is not valid.
    InvalidConfiguration,

    /// The interrupt state was not changed.
    InterruptNotChanged,

    /// The interrupt could not be triggered, asserted or de-asserted.
    InterruptNotTriggered,

    /// An error occured during interrupt allocation.
    InterruptAllocationError,

    /// An error occured during interrupt release.
    InterruptFreeError,
}

impl std::error::Error for Error {}

/// Reuse std::io::Result to simplify interoperability among crates.
pub type Result<T> = std::result::Result<T, Error>;

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Interrupt error: ")?;
        match self {
            Error::OperationNotSupported => write!(f, "operation not supported"),
            Error::InvalidConfiguration => write!(f, "invalid configuration"),
            Error::InterruptNotChanged => write!(f, "the interrupt state could not be changed"),
            Error::InterruptNotTriggered => write!(f, "the interrupt could not be triggered"),
            Error::InterruptAllocationError => write!(f, "the interrupt could not be allocated"),
            Error::InterruptFreeError => write!(f, "the interrupt could not be released"),
        }
    }
}

/// Trait used by interrupt producers to emulate an edge triggered interrupt.
///
/// This trait allows for a device to signal an interrupt event to the guest VM.
/// These events are sampled by the interrupt controller synchronously when `trigger()` is called.
///
/// Edge triggered interrupts cannot be shared.
pub trait EdgeInterrupt {
    /// Signal an interrupt to the guest VM.
    fn trigger(&self) -> Result<()>;
}

/// Trait used by interrupt producers to emulate a level triggered interrupt.
///
/// This trait allows for a device to assert an interrupt line as long as it needs service
/// from the CPU.
/// A level triggered interrupt is held asserted until the device clears the interrupt signal.
///
/// Assertion of the interrupt signal starts when `assert()` is called by the device and ends when
/// `clear()` is called by the device.
/// The object implementing this trait must hold the assertion state internally and should return
/// from `assert()` and `clear()` once the state is changed.
pub trait LevelInterrupt {
    /// Assert the interrupt line to signal an interrupt to the guest VM.
    /// This method sets the interrupt in an asserted state.
    fn assert(&self) -> Result<()>;

    /// Deassert the interrupt line to signal that the device no longer requires service.
    fn clear(&self) -> Result<()>;
}

/// Trait that allows access to a device interrupt status.
///
/// A device will implement this trait if it wants to allow other components to check its
/// interrupt status.
/// This allows implementation of auto-triggered shared level interrupts that poll the interrupt
/// state from the device in order to re-trigger the interrupt when resampled.
pub trait InterruptStatusChecker {
    /// Check if the device requires service.
    /// Returns `true` if the device has not deasserted the interrupt line and still
    /// requires service.
    fn is_active(&self) -> bool;
}

/// Trait used by interrupt controllers to configure interrupts.
///
/// An object having the `Interrupt` trait is shared between the VMM (which typically implements
/// the interrupt mechanisms) and interrupt control components.
/// It offers a control interface through the `enable()` and `disable()` methods that allow an
/// interrupt to be registered with the interrupt controllers or mechanisms in the VMM.
///
/// Objects implementing this trait are required to have internal mutability.
pub trait Interrupt {
    /// Enable generation of interrupts on this line.
    fn enable(&self) -> Result<()> {
        Err(Error::OperationNotSupported)
    }

    /// Disable generation of interrupts on this line.
    fn disable(&self) -> Result<()> {
        Err(Error::OperationNotSupported)
    }
}

/// Trait that allows interrupt controllers to configure interrupt parameters.
///
/// This enhances the control plane interface of the `Interrupt` by allowing a device to configure
/// the behavior of the interrupt.
///
/// Objects implementing this trait are required to have internal mutability.
pub trait ConfigurableInterrupt: Interrupt {
    /// Type describing the configuration spec of the interrupt.
    type Cfg;

    /// Update configuration of the interrupt.
    fn update(&self, config: &Self::Cfg) -> Result<()>;

    /// Returns the current configuration of the interrupt.
    fn get_config(&self) -> Result<Self::Cfg>;
}

/// Trait for interrupts that can be masked or unmasked.
///
/// Objects implementing this trait are required to have internal mutability.
pub trait MaskableInterrupt: Interrupt {
    /// Mask the interrupt.  Masked interrupts are remembered but
    /// not delivered.
    fn mask(&self) -> Result<()>;

    /// Unmask the interrupt, delivering it if it was pending.
    fn unmask(&self) -> Result<()>;
}

/// Trait for interrupts that can be auto-retriggered when resampled.
///
/// In some implementations of shared level-triggered interrupts the interrupt can be resampled
/// as a result of different events (e.g. an EOI) before a device explicitly deasserts the
/// interrupt. If the device still requires service, the interrupt should be reasserted.
///
/// This trait allows implementation of the interrupt mechanism described above.
/// It requires that the user of this trait configures an `InterruptStatusChecker`.
/// When the interrupt is resampled, the state of the device will be checked by the implementation
/// of this trait. If the device still requires service, the interrupt is reasserted.
///
/// An example of such mechanism is provided by KVM when using KVM_IRQFD with
/// KVM_CAP_IRQFD_RESAMPLE.
///
/// Objects implementing this trait are required to have internal mutability.
pub trait AutoRetriggerInterrupt: Interrupt {
    /// Set the `InterruptStatusChecker` object through which the interrupt can poll the device
    /// interrupt status.
    fn set_status_checker(&self, status_checker: Arc<dyn InterruptStatusChecker>) -> Result<()>;
}

/// Trait that provides access to the underlying trigger notification object used by the hypervisor.
///
/// The type of the underlying notification mechanism used by the interrupt is defined by the
/// `NotifierType` associated type.
/// This enables some use cases where the device may want to bypass the VMM completely or when the
/// device crate acts only as a control plane and the actual emulation is implemented in some other
/// component that understands the underlying mechanism.
///
/// The usage of the resulted notifier object is speciffic to the hypervisor but the semantics of
/// the object returned by the `trigger_notifier()` method should follow the semantics from
/// `EdgeInterrupt::trigger()` or `LevelInterrupt::assert()` (e.g. when the user changes the state
/// of the notifier object, an interrupt is queued for the guest).
///
/// A notable example is VFIO that allows a device to register the irqfd so that interrupts follow
/// a fast path that doesn't require going through the VMM. Another example is XEN evtchn.
///
/// Implementations of this trait must provide the trigger notifier object.
pub trait AsRefTriggerNotifier {
    /// The type of the underlying mechanism used for trigger notifications by this interrupt.
    type NotifierType;

    /// Returns a reference to a trigger notifier from this interrupt.
    ///
    /// An interrupt notifier allows for external components and processes to inject interrupts
    /// into a guest through a different interface other than `EdgeInterrupt::trigger()`.
    fn trigger_notifier(&self) -> &Self::NotifierType;
}

/// Trait that provides access to the underlying resample notification object used by
/// the hypervisor.
///
/// This enables use cases where the notification that the interrupt was resampled is
/// handled by a component that understands the underlying hypervisor interrupt implementation
/// and wants to bypass the VMM.
///
/// The semantics of the object returned by `resample_notifier()` are similar to those of
/// `AutoRetriggerInterrupt` (when the state of the notifier object changes it means that
/// the interrupt was resampled and the device should reassert the interrupt).
///
/// VFIO supports the registration of a `resamplefd` which would be returned by
/// `resample_notifier`.
///
/// Implementations of this trait must provide the resample notifier object.
pub trait AsRefResampleNotifier {
    /// The type of the underlying mechanism used for resample notifications by an interrupt.
    type NotifierType;

    /// Returns a reference to a resample notifier from an interrupt.
    ///
    /// An end-of-interrupt notifier allows for external components and processes to be notified
    /// when a guest acknowledges an interrupt. This can be used to resample and inject a
    /// level-triggered interrupt, or to mitigate the effect of lost timer interrupts.
    fn resample_notifier(&self) -> &Self::NotifierType;
}

/// Trait to manage a group of interrupt sources for a device.
///
/// A device may use an InterruptSourceGroup to manage multiple interrupts of the same type.
/// The group allows a device to request and release interrupts and perform actions on the
/// whole collection of interrupts like enable and disable for cases where enabling or disabling
/// a single interrupt in the group does not make sense. For example, PCI MSI interrupts must be
/// enabled as a group.
pub trait InterruptSourceGroup: Send {
    /// Type of the interrupts contained in this group.
    type InterruptType: Interrupt;

    /// Interrupt Type returned by get
    type InterruptWrapper: Deref<Target = Self::InterruptType>;

    /// Return whether the group manages no interrupts.
    fn is_empty(&self) -> bool;

    /// Get number of interrupt sources managed by the group.
    fn len(&self) -> usize;

    /// Enable the interrupt sources in the group to generate interrupts.
    fn enable(&self) -> Result<()>;

    /// Disable the interrupt sources in the group to generate interrupts.
    fn disable(&self) -> Result<()>;

    /// Return the index-th interrupt in the group, or `None` if the index is out
    /// of bounds.
    fn get(&self, index: usize) -> Option<Self::InterruptWrapper>;

    /// Request new interrupts within this group.
    fn allocate_interrupts(&mut self, size: usize) -> Result<()>;

    /// Release all interrupts within this group.
    fn free_interrupts(&mut self) -> Result<()>;
}
