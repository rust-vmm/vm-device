// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

use std::cmp::Ordering;
use std::convert::TryFrom;
use std::ops::{Add, Sub};

/// This trait defines the operations we expect to apply to bus address values.
pub trait BusAddress:
    Add<<Self as BusAddress>::V, Output = Self>
    + Copy
    + Eq
    + Ord
    + Sub<Output = <Self as BusAddress>::V>
{
    /// Defines the underlying value type of the `BusAddress`.
    type V: Add<Output = Self::V>
        + Copy
        + From<u8>
        + PartialEq
        + Ord
        + Sub<Output = Self::V>
        + TryFrom<usize>;

    /// Return the inner value.
    fn value(&self) -> Self::V;

    /// Return the bus address computed by offsetting `self` by the specified value, if no
    /// overflow occurs.
    fn checked_add(&self, value: Self::V) -> Option<Self>;
}

/// Represents a MMIO address.
#[derive(Clone, Copy, Debug)]
pub struct MmioAddress(pub u64);

/// This type defines the underlying value type for PIO addresses, which might be different
/// for different platforms.
pub type PioAddressValue = u16;

/// Represents a PIO address.
#[derive(Clone, Copy, Debug)]
pub struct PioAddress(pub PioAddressValue);

// Implementing `BusAddress` and its prerequisites for `MmioAddress`.

impl PartialEq for MmioAddress {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for MmioAddress {}

impl PartialOrd for MmioAddress {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl Ord for MmioAddress {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl Add<u64> for MmioAddress {
    type Output = Self;

    fn add(self, rhs: u64) -> Self::Output {
        MmioAddress(self.0 + rhs)
    }
}

impl Sub for MmioAddress {
    type Output = u64;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0 - rhs.0
    }
}

impl BusAddress for MmioAddress {
    type V = u64;

    fn value(&self) -> Self::V {
        self.0
    }

    fn checked_add(&self, value: Self::V) -> Option<Self> {
        self.0.checked_add(value).map(MmioAddress)
    }
}

// Implementing `BusAddress` and its prerequisites for `PioAddress`.

impl PartialEq for PioAddress {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for PioAddress {}

impl PartialOrd for PioAddress {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl Ord for PioAddress {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl Add<PioAddressValue> for PioAddress {
    type Output = Self;

    fn add(self, rhs: PioAddressValue) -> Self::Output {
        PioAddress(self.0 + rhs)
    }
}

impl Sub for PioAddress {
    type Output = PioAddressValue;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0 - rhs.0
    }
}

impl BusAddress for PioAddress {
    type V = PioAddressValue;

    fn value(&self) -> Self::V {
        self.0
    }

    fn checked_add(&self, value: Self::V) -> Option<Self> {
        self.0.checked_add(value).map(PioAddress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fmt::Debug;

    // `addr_zero` should be an address equivalent to 0, while `max_value` should contain the
    // maximum possible address value.
    fn check_bus_address_ops<A>(addr_zero: A, max_value: A::V)
    where
        A: BusAddress + Debug,
        A::V: Debug,
    {
        let value = A::V::from(5);
        let addr = addr_zero + value;

        assert_eq!(addr, addr);
        assert!(addr_zero < addr);
        assert_eq!(addr - addr_zero, value);

        assert_eq!(addr.value(), value);
        assert_eq!(addr_zero.checked_add(value).unwrap(), addr);

        let addr_max = addr_zero.checked_add(max_value).unwrap();
        assert!(addr_max.checked_add(A::V::from(1)).is_none());
    }

    #[test]
    fn test_address_ops() {
        check_bus_address_ops(MmioAddress(0), std::u64::MAX);
        check_bus_address_ops(PioAddress(0), std::u16::MAX);
    }
}
