// Copyright 2018 The Chromium OS Authors. All rights reserved.
// Copyright © 2019 Intel Corporation
//
// Portions Copyright 2017 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE-BSD-3-Clause file.
//
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

use vm_memory::{GuestAddress, GuestUsize};

use std::result;

/// Errors associated with system resources allocation.
#[derive(Debug)]
pub enum Error {
    /// The allocator already exists when adding a new one.
    AllocatorExist,
    /// The allocator doesn't exists when trying to allocate.
    AllocatorNotExist,
    /// Unsigned integer resource allocate fails.
    IdAllocateError(crate::resource::Error),
    /// Address resource allocate fails.
    AddressAllocateError(crate::resource::Error),
}

pub type Result<T> = result::Result<T, Error>;

/// The parameters that integer resource allocation needs.
pub struct IdAllocateParameters {
    /// Resource to be allocated or freed.
    pub resource: Option<u32>,
    /// Specify resource to be allocated or freed.
    pub allocate: bool,
}

/// The parameters that address resource allocation needs.
pub struct AddrAllocateParameters {
    /// Resource to be allocated or freed.
    pub resource: Option<GuestAddress>,
    /// Size to be allocated or freed.
    pub size: GuestUsize,
    /// Specify resource to be allocated or freed.
    pub allocate: bool,
}

/// Integer resource allocation callback type.
pub type IdAllocateFunc = Box<Fn(IdAllocateParameters) -> Result<(u32)>>;

/// Address resource allocation callback type.
pub type AddrAllocateFunc = Box<Fn(AddrAllocateParameters) -> Result<(GuestAddress)>>;

/// System level allocator trait for VMM.
pub trait SystemAllocator {
    /// Integer resource allocator callback.
    fn device_id_cb(&mut self) -> Option<IdAllocateFunc> {
        None
    }

    /// Integer resource allocator callback.
    fn irq_cb(&mut self) -> Option<IdAllocateFunc> {
        None
    }

    /// Port IO address resource allocator callback.
    fn pio_addr_cb(&mut self) -> Option<AddrAllocateFunc> {
        None
    }

    /// MmiO address resource allocator callback.
    fn mmio_addr_cb(&mut self) -> Option<AddrAllocateFunc> {
        None
    }
}
