// Copyright (C) 2021 Amazon.com, Inc. or its affiliates.
// All Rights Reserved.

// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

//! Traits and Structs to manage MSI interrupt sources for devices.
//!
//! MSI interrupts are typically used by PCI devices.
//! These structs and traits can be used to configure both MSI and MSIX interrupts.

use crate::interrupt::{ConfigurableInterrupt, MaskableInterrupt};

/// Configuration data for MSI/MSI-X interrupts.
///
/// On x86 platforms, these interrupts are vectors delivered directly to the LAPIC.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct MsiIrqConfig {
    /// High address to delivery message signaled interrupt.
    pub high_addr: u32,
    /// Low address to delivery message signaled interrupt.
    pub low_addr: u32,
    /// Data to write to delivery message signaled interrupt.
    pub data: u32,
    /// Unique ID of the device to delivery message signaled interrupt.
    pub devid: u32,
}

/// Trait for defining properties of MSI interrupts.
pub trait MsiInterrupt: ConfigurableInterrupt<Cfg = MsiIrqConfig> + MaskableInterrupt {}

/// Blanket implementation for Interrupts that use a MsiIrqConfig.
impl<T> MsiInterrupt for T where T: ConfigurableInterrupt<Cfg = MsiIrqConfig> + MaskableInterrupt {}
