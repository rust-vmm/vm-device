// Copyright (C) 2019 Alibaba Cloud. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Helper utilities for handling MSI interrupts.

use super::*;
use kvm_bindings::{kvm_irq_routing_entry, KVM_IRQ_ROUTING_MSI};

pub(super) struct MsiConfig {
    pub(super) irqfd: EventFd,
    pub(super) config: Mutex<MsiIrqSourceConfig>,
}

impl MsiConfig {
    pub(super) fn new() -> Self {
        MsiConfig {
            irqfd: EventFd::new(0).unwrap(),
            config: Mutex::new(Default::default()),
        }
    }
}

pub(super) fn new_msi_routing_entry(
    gsi: InterruptIndex,
    msicfg: &MsiIrqSourceConfig,
) -> kvm_irq_routing_entry {
    let mut entry = kvm_irq_routing_entry {
        gsi,
        type_: KVM_IRQ_ROUTING_MSI,
        flags: 0,
        ..Default::default()
    };
    unsafe {
        entry.u.msi.address_hi = msicfg.high_addr;
        entry.u.msi.address_lo = msicfg.low_addr;
        entry.u.msi.data = msicfg.data;
    }
    entry
}

pub(super) fn create_msi_routing_entries(
    base: InterruptIndex,
    configs: &[InterruptSourceConfig],
) -> Result<Vec<kvm_irq_routing_entry>> {
    let _ = base
        .checked_add(configs.len() as u32)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    let mut entries = Vec::with_capacity(configs.len());
    for (i, ref val) in configs.iter().enumerate() {
        if let InterruptSourceConfig::MsiIrq(msicfg) = val {
            let entry = new_msi_routing_entry(base + i as u32, msicfg);
            entries.push(entry);
        } else {
            return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        }
    }
    Ok(entries)
}
