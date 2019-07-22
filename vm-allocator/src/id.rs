// Copyright 2018 The Chromium OS Authors. All rights reserved.
// Copyright © 2019 Intel Corporation
//
// Portions Copyright 2017 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE-BSD-3-Clause file.
//
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

use std::result;

use crate::resource::{Error, Resource, ResourceAllocator, ResourceSize};

pub type Result<T> = result::Result<T, Error>;

impl Resource for u32 {}
impl ResourceSize for u32 {}

/// Manages allocating unsigned integer resources.
/// Use `IdAllocator` whenever a unique unsigned 32-bit number needs to be allocated.
///
/// # Arguments
///
/// * `start` - The starting integer to manage.
/// * `end` - The ending integer to manage.
/// * `used` - The used integer ordered from lowest to highest.
///
/// # Examples
///
/// ```
/// use vm_allocator::IdAllocator;
/// use vm_allocator::ResourceAllocator;
///
/// IdAllocator::new(1, std::u32::MAX).map(|mut p| {
///     assert_eq!(p.allocate(Some(1), 1).unwrap(), 1);
///     assert_eq!(p.allocate(Some(3), 1).unwrap(), 3);
/// });
///
/// ```
#[derive(Debug)]
pub struct IdAllocator {
    start: u32,
    end: u32,
    used: Vec<u32>,
}

impl IdAllocator {
    /// Creates a new `IdAllocator` for managing a range of unsigned integer.
    pub fn new(start: u32, end: u32) -> Option<Self> {
        Some(IdAllocator {
            start,
            end,
            used: Vec::new(),
        })
    }

    fn first_usable_number(&self) -> Result<u32> {
        if self.used.is_empty() {
            return Ok(self.start);
        }

        let mut previous = self.start;

        for iter in self.used.iter() {
            if *iter > previous {
                return Ok(previous);
            } else {
                match iter.checked_add(1) {
                    Some(p) => previous = p,
                    None => return Err(Error::Overflow),
                }
            }
        }
        if previous <= self.end {
            Ok(previous)
        } else {
            Err(Error::Overflow)
        }
    }
}

impl ResourceAllocator<u32, u32> for IdAllocator {
    fn allocate(&mut self, resource: Option<u32>, size: u32) -> Result<u32> {
        // `size` should be 1 because id resource allocation request can be
        // non continuous or continuous.
        if size != 1 || size == 0 {
            return Err(Error::SizeInvalid);
        }
        let ret = match resource {
            // Specified id resource to be allocated.
            Some(res) => {
                if res < self.start || res > self.end {
                    return Err(Error::OutofScope);
                }
                match self.used.iter().find(|&&x| x == res) {
                    Some(_) => {
                        return Err(Error::Duplicated);
                    }
                    None => res,
                }
            }
            None => self.first_usable_number()?,
        };
        self.used.push(ret);
        self.used.sort();
        Ok(ret)
    }

    /// Free an already allocated id and will keep the order.
    fn free(&mut self, res: u32, size: u32) {
        // Only support free a singal resource.
        if size != 1 || size == 0 {
            return;
        }
        if let Ok(idx) = self.used.binary_search(&res) {
            self.used.remove(idx);
        }
    }
}
