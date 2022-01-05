// Copyright (C) 2021 Amazon.com, Inc. or its affiliates.
// All Rights Reserved.

// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

//! Traits and Structs to manage legacy interrupt sources for devices.
//!
//! Legacy interrupt sources typically include pin based interrupt lines.

use crate::interrupt::ConfigurableInterrupt;

/// Definition for PCI INTx pins.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd)]
pub enum IntXPin {
    /// INTA
    IntA = 0x1,
    /// INTB
    IntB = 0x2,
    /// INTC
    IntC = 0x3,
    /// INTD
    IntD = 0x4,
}

/// Standard configuration for Legacy interrupts.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct LegacyIrqConfig {
    /// Input of the system interrupt controllers the device's interrupt pin is connected to.
    /// Implemented by any device that makes use of an interrupt pin.
    pub interrupt_line: Option<u32>,
    /// Specifies which interrupt pin the device uses.
    pub interrupt_pin: Option<IntXPin>,
}

/// Trait for defining properties of Legacy interrupts.
pub trait LegacyInterrupt: ConfigurableInterrupt<Cfg = LegacyIrqConfig> {}

/// Blanket implementation for Interrupts that use a LegacyIrqConfig.
impl<T> LegacyInterrupt for T where T: ConfigurableInterrupt<Cfg = LegacyIrqConfig> {}
