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
//! Nowadays, IRQ lines is not the only mechanism to delivery device interrupts to processors.
//! MSI [(Message Signalled Interrupt)](https://en.wikipedia.org/wiki/Message_Signaled_Interrupts)
//! is another commonly used alternative in-band method of signaling an interrupt, using special
//! in-band messages to replace traditional out-of-band assertion of dedicated interrupt lines.
//! While more complex to implement in a device, message signaled interrupts have some significant
//! advantages over pin-based out-of-band interrupt signaling. Message signaled interrupts are
//! supported in PCI bus since its version 2.2, and in later available PCI Express bus. Some non-PCI
//! architectures also use message signaled interrupts.
//!
//! On the other hand, IRQ is a term commonly used by Operating Systems when dealing with hardware
//! interrupts. The IRQ number managed by OSes is independent of the IRQ number managed by VMM.
//! To make things easy, `Interrupt Source` is used instead of IRQ for pin-based interrupts and
//! MSI interrupts.
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
//!
//! The overall flow to deal with interrupts is:
//! * the VMM creates an interrupt manager
//! * the VMM creates a device manager, passing on an reference to the interrupt manager
//! * the device manager passes on an reference to the interrupt manager to all registered devices
//! * guest kernel loads drivers for virtual devices
//! * guest device driver determines the type and number of interrupts needed, and update the
//!   device configuration
//! * the virtual device backend requests the interrupt manager to create an interrupt group
//!   according to guest configuration information

use std::sync::Arc;
use vmm_sys_util::eventfd::EventFd;

/// Reuse std::io::Result to simplify inter operability.
pub type Result<T> = std::io::Result<T>;

/// Data type to store an interrupt index.
pub type InterruptIndex = u32;

#[cfg(feature = "generic_msi")]
/// Maximum number of generic Message Signalled Interrupts supported per device.
pub const MAX_GENERIC_MSI_IRQS: InterruptIndex = 32;

/// Type of interrupt source.
pub enum InterruptSourceType {
    #[cfg(feature = "legacy_irq")]
    /// Legacy Pin-based Interrupt.
    /// On x86 platforms, legacy interrupts are routed through 82599 PICs and/or IOAPICs.
    Legacy,
    #[cfg(feature = "generic_msi")]
    /// Generic Message Signalled Interrupt.
    /// Some non-PCI devices (like HPET on x86) make use of generic MSI in platform specific ways.
    GenericMsi,
    #[cfg(feature = "pci_msi")]
    /// PCI Message Signalled Interrupt.
    PciMsi,
    #[cfg(feature = "pci_msix")]
    /// PCI Message Signalled Interrupt - Second Generation.
    PciMsix,
}

/// Configuration data for an interrupt source.
pub enum InterruptSourceConfig {
    /// Configuration data for Legacy interrupts.
    Legacy(LegacyIrqSourceConfig),
    /// Configuration data for GenericMsi, PciMsi, PciMsix interrupts.
    Msi(MsiIrqSourceConfig),
}

/// Configuration data for legacy interrupts.
///
/// On x86 platforms, legacy interrupts means those interrupts routed through PICs or IOAPICs.
pub struct LegacyIrqSourceConfig {
    /// Bitmask for valid bits in the corresponding interrupt status register.
    pub valid_flag_mask: u32,
}

/// Configuration data for GenericMsi, PciMsi, PciMsix interrupts.
pub struct MsiIrqSourceConfig {
    pub high_addr: u32,
    pub low_addr: u32,
    pub data: u32,
}

/// Trait to manage [InterruptSourceGroup](trait.InterruptSourceGroup.html) objects for devices.
pub trait InterruptManager {
    /// Create an [InterruptSourceGroup](trait.InterruptSourceGroup.html) object for a device.
    ///
    /// An [InterruptSourceGroup](trait.InterruptSourceGroup.html) object manages all interrupt
    /// sources of the same type for a device.
    ///
    /// # Arguments
    /// * ty: type of interrupt source.
    /// * base: base Interrupt Source ID to be managed by the group object.
    /// * count: number of Interrupt Sources to be managed by the group object.
    fn create_group(
        &self,
        ty: InterruptSourceType,
        base: InterruptIndex,
        count: InterruptIndex,
    ) -> Result<Arc<Box<dyn InterruptSourceGroup>>>;

    /// Destroy an [InterruptSourceGroup](trait.InterruptSourceGroup.html) object created by
    /// [create_group()](trait.InterruptManager.html#tymethod.create_group).
    fn destroy_group(&self, group: Arc<Box<dyn InterruptSourceGroup>>) -> Result<()>;

    /*
    /// Enable all created interrupt groups.
    ///
    /// During early startup stages, the manager may get called to create interrupt groups when
    /// the underline system is not ready to handle interrupts yet. So the manager will record
    /// those requests and apply them when the underline system is ready.
    fn start(&self) -> Result<()>;
    */
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
/// [InterruptSourceGroup](trait.InterruptSourceGroup.html) object, instead of abstracting each
/// interrupt source as a distinct InterruptSource.
#[allow(clippy::len_without_is_empty)]
pub trait InterruptSourceGroup {
    /// Get type of the interrupt group.
    fn get_type(&self) -> InterruptSourceType;

    /// Get number of assigned Interrupt Sources.
    fn len(&self) -> InterruptIndex;

    /// Get base of assigned Interrupt Source Identifier.
    fn get_base(&self) -> InterruptIndex;

    /// Get the irqfd to inject interrupts into the guest by using Linux KVM module.
    fn get_irqfd(&self, _index: InterruptIndex) -> Option<&EventFd> {
        None
    }

    /// Configure the interrupt source for generating interrupts for guest.
    ///
    /// # Arguments
    /// * index: sub-index into the group.
    /// * config: configuration data for the interrupt source.
    fn configure(&self, index: InterruptIndex, config: InterruptSourceConfig) -> Result<()>;

    //fn set_mask(&self, index: InterruptIndex, masked: bool) -> Result<()>;

    /// Inject an interrupt into the guest.
    ///
    /// If the interrupt has an associated `interrupt_status` register, all bits set in `flag`
    /// will be atomically ORed into the `interrupt_status` register.
    fn trigger(&self, index: InterruptIndex, flags: u32) -> Result<()>;

    /// Acknowledge that the guest has handled the interrupt.
    ///
    /// If the interrupt has an associated `interrupt_status` register, all bits set in `flag`
    /// will get atomically cleared from the `interrupt_status` register.
    fn ack(&self, index: InterruptIndex, flags: u32) -> Result<()>;
}

#[cfg(feature = "kvm_irq")]
mod kvm_irq;
#[cfg(feature = "kvm_irq")]
pub use kvm_irq::KvmIrqManager;
