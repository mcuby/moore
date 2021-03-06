// Copyright (c) 2017 Fabian Schuiki

//! This module implements constant values for VHDL.

#![deny(missing_docs)]

mod traits;
mod integer;
mod floating;
mod arena;

pub use self::traits::*;
pub use self::integer::*;
pub use self::floating::*;
pub use self::arena::*;
