//! Core math, Num type, IDs, time and errors.

pub mod handle;
pub mod num;
pub mod slab;

pub use handle::{Handle, RawHandle};
pub use num::Num;
pub use slab::Slab;
