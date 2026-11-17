//! Core math, Num type, IDs, time and errors.

pub mod error;
pub mod guid;
pub mod handle;
pub mod math;
pub mod num;
pub mod slab;
pub mod time;

pub use error::{Error, Result};
pub use guid::{Guid, ParseGuidError};
pub use handle::{Handle, RawHandle};
pub use math::{Color, ParseColorError, Rect};
pub use num::{Notation, Num, NumFormat, ParseNumError};
pub use slab::Slab;
pub use time::{Advance, FixedClock, Time};
