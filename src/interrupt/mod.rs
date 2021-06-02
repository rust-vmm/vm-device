// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause

use std::result::Result;

/// Abstraction for a simple, push-button like interrupt mechanism.
pub trait Trigger {
    /// Underlying type for the potential error conditions returned by `Self::trigger`.
    type E;

    /// Trigger an event.
    fn trigger(&self) -> Result<(), Self::E>;
}
