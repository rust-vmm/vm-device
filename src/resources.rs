// Copyright (C) 2019 Alibaba Cloud. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Structs to manage device resources.

/// Type of Message Singaled Interrupt
#[derive(Copy, Clone, PartialEq)]
pub enum MsiIrqType {
    /// PCI MSI IRQ numbers.
    PciMsi,
    /// PCI MSIx IRQ numbers.
    PciMsix,
    /// Generic MSI IRQ numbers.
    GenericMsi,
}

/// Enumeration for device resources.
#[allow(missing_docs)]
#[derive(Clone)]
pub enum Resource {
    /// IO Port address range.
    PioAddressRange { base: u16, size: u16 },
    /// Memory Mapped IO address range.
    MmioAddressRange { base: u64, size: u64 },
    /// Legacy IRQ number.
    LegacyIrq(u32),
    /// Message Signaled Interrupt
    MsiIrq {
        ty: MsiIrqType,
        base: u32,
        size: u32,
    },
    /// Network Interface Card MAC address.
    MacAddresss(String),
    /// KVM memslot index.
    KvmMemSlot(u32),
}

/// Newtype to store a set of device resources.
#[derive(Default, Clone)]
pub struct DeviceResources(Vec<Resource>);

impl DeviceResources {
    /// Create a container object to store device resources.
    pub fn new() -> Self {
        DeviceResources(Vec::new())
    }

    /// Append a device resource to the container object.
    pub fn append(&mut self, entry: Resource) {
        self.0.push(entry);
    }

    /// Get the IO port address resources.
    pub fn get_pio_address_ranges(&self) -> Vec<(u16, u16)> {
        let mut vec = Vec::new();
        for entry in self.0.iter().as_ref() {
            if let Resource::PioAddressRange { base, size } = entry {
                vec.push((*base, *size));
            }
        }
        vec
    }

    /// Get the Memory Mapped IO address resources.
    pub fn get_mmio_address_ranges(&self) -> Vec<(u64, u64)> {
        let mut vec = Vec::new();
        for entry in self.0.iter().as_ref() {
            if let Resource::MmioAddressRange { base, size } = entry {
                vec.push((*base, *size));
            }
        }
        vec
    }

    /// Get the first legacy interrupt number(IRQ).
    pub fn get_legacy_irq(&self) -> Option<u32> {
        for entry in self.0.iter().as_ref() {
            if let Resource::LegacyIrq(base) = entry {
                return Some(*base);
            }
        }
        None
    }

    /// Get information about the first PCI MSI interrupt resource.
    pub fn get_pci_msi_irqs(&self) -> Option<(u32, u32)> {
        self.get_msi_irqs(MsiIrqType::PciMsi)
    }

    /// Get information about the first PCI MSIx interrupt resource.
    pub fn get_pci_msix_irqs(&self) -> Option<(u32, u32)> {
        self.get_msi_irqs(MsiIrqType::PciMsix)
    }

    /// Get information about the first Generic MSI interrupt resource.
    pub fn get_generic_msi_irqs(&self) -> Option<(u32, u32)> {
        self.get_msi_irqs(MsiIrqType::GenericMsi)
    }

    fn get_msi_irqs(&self, ty: MsiIrqType) -> Option<(u32, u32)> {
        for entry in self.0.iter().as_ref() {
            if let Resource::MsiIrq {
                ty: msi_type,
                base,
                size,
            } = entry
            {
                if ty == *msi_type {
                    return Some((*base, *size));
                }
            }
        }
        None
    }

    /// Get the KVM memory slots to map memory into the guest.
    pub fn get_kvm_mem_slots(&self) -> Vec<u32> {
        let mut vec = Vec::new();
        for entry in self.0.iter().as_ref() {
            if let Resource::KvmMemSlot(index) = entry {
                vec.push(*index);
            }
        }
        vec
    }

    /// Get the first resource information for NIC MAC address.
    pub fn get_mac_address(&self) -> Option<String> {
        for entry in self.0.iter().as_ref() {
            if let Resource::MacAddresss(addr) = entry {
                return Some(addr.clone());
            }
        }
        None
    }

    /// Get immutable reference to all the resources.
    pub fn get_all_resources(&self) -> &[Resource] {
        &self.0
    }
}
