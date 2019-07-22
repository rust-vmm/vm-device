// Copyright 2018 The Chromium OS Authors. All rights reserved.
// Copyright © 2019 Intel Corporation
//
// Portions Copyright 2017 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE-BSD-3-Clause file.
//
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

use std::fmt::{self, Display};

/// Define Error list for `ResourceAllocator` trait.
#[derive(Debug)]
pub enum Error {
    /// The resource allocation failed because request is out os scope.
    OutofScope,
    /// The resource allocation failed because request is overflow.
    Overflow,
    /// The resource allocation failed because request scope is overlap.
    Overlap,
    /// The resource allocation failed because request is duplicated.
    Duplicated,
    /// The resource allocation failed because of unaligned address request.
    UnalignedAddress,
    /// The resource allocation failed because request size is invalid.
    SizeInvalid,
}

impl Display for Error {
    // This trait requires `fmt` with this exact signature.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Error::*;

        match self {
            OutofScope => write!(f, "Resource being allocated is out of scope"),
            Overflow => write!(f, "Resource being allocated is overflow"),
            Overlap => write!(f, "Resource being allocated has overlap"),
            Duplicated => write!(f, "Resource being allocated is duplicated"),
            UnalignedAddress => write!(f, "Resource being allocated is unaligned"),
            SizeInvalid => write!(f, "Resource allocation request size is invalid"),
        }
    }
}

/// Trait for resource types.
pub trait Resource {}

/// Trait for resource size.
pub trait ResourceSize {}

/// Allocator trait with basic allocate and free functions.
pub trait ResourceAllocator<T: Resource, S: ResourceSize> {
    /// Allocate some resource with given `resource` and `size`.
    ///
    /// # Arguments
    ///
    /// * `resource`: resource to be allocate.
    /// * `size`: resource size of allocation request.
    ///
    fn allocate(&mut self, resource: Option<T>, size: S) -> Result<T, Error>;

    /// Free resource specified by given `resource` and `size`.
    ///
    /// # Arguments
    ///
    /// * `resource`: resource to be free.
    /// * `size`: resource size of free request.
    ///
    fn free(&mut self, resource: T, size: S);
}
