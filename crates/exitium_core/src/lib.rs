//! Core math, Num type, IDs, time and errors.

pub mod error;
pub mod guid;
pub mod handle;
pub mod num;
pub mod slab;

pub use error::{Error, Result};
pub use guid::{Guid, ParseGuidError};
pub use handle::{Handle, RawHandle};
pub use num::Num;
pub use slab::Slab;
