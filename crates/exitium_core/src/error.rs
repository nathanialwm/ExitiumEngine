//! [`Error`], the engine-wide error type, and its [`Result`] alias.
//!
//! # Crate-local errors come first
//!
//! A fallible function returns the narrowest error type that describes its
//! failures, defined in its own crate: [`ParseGuidError`] here, a query error
//! in `exitium_ecs`, and so on. The caller can then match on exactly what can
//! go wrong, and the error stays small and cheap (usually `Copy`, never
//! allocated), which matters on hot paths.
//!
//! # When to use `Error`
//!
//! Use [`Error`] where failures from several crates meet and the caller
//! reports or propagates them instead of handling each one: app and plugin
//! setup, asset and save loading, editor and MCP command handlers, tools, and
//! game code such as `fn main() -> exitium_core::Result<()>`.
//!
//! Do not use it on hot paths, since building one can allocate. Do not use it
//! either when a caller has to react to one specific failure: return the
//! crate-local error there, and let the boundary convert it.
//!
//! # Converting with `?`
//!
//! `?` turns any error that has a `From` impl into an `Error`. This crate
//! provides `From` for its own errors and for [`std::io::Error`]. It cannot
//! name the error types of the crates that depend on it, so each of those
//! crates adds `impl From<ItsError> for exitium_core::Error` itself (the orphan
//! rule allows it, because `ItsError` is local there), usually as
//! `Error::other(err)`. Without such an impl, call [`Error::other`] at the
//! boundary.
//!
//! # Display and source
//!
//! Every variant is a transparent wrapper. [`Display`](fmt::Display) prints the
//! wrapped error's message, and [`source`](std::error::Error::source) returns
//! the wrapped error's own source. A report that walks the source chain
//! therefore prints each message once.
//!
//! ```
//! use exitium_core::{Error, Guid, Result};
//!
//! fn asset_id(text: &str) -> Result<Guid> {
//!     let guid: Guid = text.parse()?; // ParseGuidError converts into Error
//!     if guid.is_nil() {
//!         return Err(Error::other("the nil GUID names no asset"));
//!     }
//!     Ok(guid)
//! }
//!
//! assert!(asset_id("67e55044-10b1-426f-9247-bb680e5fe0c8").is_ok());
//! assert!(matches!(asset_id("ship.png"), Err(Error::ParseGuid(_))));
//! let nil = asset_id("00000000-0000-0000-0000-000000000000").unwrap_err();
//! assert_eq!(nil.to_string(), "the nil GUID names no asset");
//! ```

use std::error::Error as StdError;
use std::fmt;
use std::io;

use crate::guid::ParseGuidError;
use crate::math::ParseColorError;
use crate::num::ParseNumError;

/// An error at a crate boundary. The [module docs](crate::error) say when to
/// use it instead of a crate-local error.
///
/// New variants may be added later, so a `match` on it needs a `_` arm.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A text that should be a [`Guid`](crate::Guid) is not one.
    ParseGuid(ParseGuidError),
    /// A text that should be a hex [`Color`](crate::Color) is not one.
    ParseColor(ParseColorError),
    /// A text that should be a [`Num`](crate::Num) is not one.
    ParseNum(ParseNumError),
    /// A file, stream or other I/O operation failed.
    Io(io::Error),
    /// Any other error or message, boxed whole. Build it with
    /// [`Error::other`].
    Other(Box<dyn StdError + Send + Sync + 'static>),
}

/// [`std::result::Result`] with [`Error`] as the default error type.
///
/// The second parameter keeps `Result<T, E>` usable for other error types in
/// code that imports this alias in place of the prelude's `Result`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// Boxes `error` into [`Error::Other`]. It takes any error type that is
    /// `Send + Sync + 'static`, or a message as a `&str` or `String`.
    ///
    /// The boxed error keeps its type, so a caller that matches
    /// `Error::Other(inner)` can still `inner.downcast_ref::<ItsError>()`.
    #[must_use]
    pub fn other<E>(error: E) -> Self
    where
        E: Into<Box<dyn StdError + Send + Sync + 'static>>,
    {
        Self::Other(error.into())
    }
}

impl fmt::Display for Error {
    /// Prints the wrapped error's message, unchanged.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseGuid(error) => fmt::Display::fmt(error, f),
            Self::ParseColor(error) => fmt::Display::fmt(error, f),
            Self::ParseNum(error) => fmt::Display::fmt(error, f),
            Self::Io(error) => fmt::Display::fmt(error, f),
            Self::Other(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl StdError for Error {
    /// The wrapped error's own source, not the wrapped error itself:
    /// [`Display`](fmt::Display) already printed that one.
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::ParseGuid(error) => error.source(),
            Self::ParseColor(error) => error.source(),
            Self::ParseNum(error) => error.source(),
            Self::Io(error) => error.source(),
            Self::Other(error) => error.source(),
        }
    }
}

impl From<ParseGuidError> for Error {
    #[inline]
    fn from(error: ParseGuidError) -> Self {
        Self::ParseGuid(error)
    }
}

impl From<ParseColorError> for Error {
    #[inline]
    fn from(error: ParseColorError) -> Self {
        Self::ParseColor(error)
    }
}

impl From<ParseNumError> for Error {
    #[inline]
    fn from(error: ParseNumError) -> Self {
        Self::ParseNum(error)
    }
}

impl From<io::Error> for Error {
    #[inline]
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Guid, Num};

    /// An error with a source, to check what `Error` forwards.
    #[derive(Debug)]
    struct SaveFailed(io::Error);

    impl fmt::Display for SaveFailed {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("saving slot 2 failed")
        }
    }

    impl StdError for SaveFailed {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            Some(&self.0)
        }
    }

    fn save_failed() -> SaveFailed {
        SaveFailed(io::Error::other("disk unplugged"))
    }

    /// The message of `error` and of each source after it, outermost first.
    fn report(error: &dyn StdError) -> Vec<String> {
        let mut messages = vec![error.to_string()];
        let mut source = error.source();
        while let Some(next) = source {
            messages.push(next.to_string());
            source = next.source();
        }
        messages
    }

    #[test]
    fn question_mark_converts_core_and_io_errors() {
        // `Result<T, E>` with an explicit `E` still works next to the alias.
        fn parse(text: &str) -> Result<Guid, ParseGuidError> {
            text.parse()
        }
        fn load(text: &str) -> Result<Guid> {
            Ok(parse(text)?)
        }
        fn read() -> Result<u8> {
            let read: io::Result<u8> = Err(io::Error::from(io::ErrorKind::NotFound));
            Ok(read?)
        }
        fn tint(text: &str) -> Result<Color> {
            Ok(Color::from_hex(text)?)
        }
        fn cost(text: &str) -> Result<Num> {
            Ok(Num::parse(text)?)
        }

        assert!(matches!(
            load("nope"),
            Err(Error::ParseGuid(ParseGuidError::InvalidLength { len: 4 }))
        ));
        assert!(matches!(read(), Err(Error::Io(e)) if e.kind() == io::ErrorKind::NotFound));
        let color = ParseColorError::InvalidLength { len: 2 };
        let error = tint("#ff").unwrap_err();
        assert!(matches!(error, Error::ParseColor(e) if e == color));
        assert_eq!(report(&error), [color.to_string()]);
        assert!(matches!(cost("1.5K"), Err(Error::ParseNum(ParseNumError))));
        assert_eq!(cost("1.5e3").ok(), Some(Num::from(1500)));
    }

    #[test]
    fn other_wraps_messages_and_errors() {
        let error = Error::other("asset `ship.png` is missing");
        assert!(matches!(error, Error::Other(_)));
        assert_eq!(error.to_string(), "asset `ship.png` is missing");

        let error = Error::other(format!("{} entities left", 3));
        assert_eq!(error.to_string(), "3 entities left");

        // The boxed error keeps its type.
        let wrapped = ParseGuidError::InvalidLength { len: 3 };
        let error = Error::other(wrapped);
        let Error::Other(inner) = &error else {
            panic!("expected Error::Other, got {error:?}");
        };
        assert_eq!(inner.downcast_ref::<ParseGuidError>(), Some(&wrapped));
    }

    #[test]
    fn every_message_is_reported_once() {
        let parse = ParseGuidError::InvalidLength { len: 3 };
        assert_eq!(report(&Error::from(parse)), [parse.to_string()]);
        let parse = ParseNumError;
        assert_eq!(report(&Error::from(parse)), [parse.to_string()]);
        assert_eq!(report(&Error::other("plain message")), ["plain message"]);
        // `io::Error` is itself transparent over a custom error, so both paths
        // give the same report.
        let expected = ["saving slot 2 failed", "disk unplugged"];
        assert_eq!(report(&Error::other(save_failed())), expected);
        assert_eq!(
            report(&Error::from(io::Error::other(save_failed()))),
            expected
        );
    }

    #[test]
    fn error_can_cross_threads() {
        fn assert_thread_safe<T: Send + Sync + 'static>() {}
        assert_thread_safe::<Error>();

        let boxed: Box<dyn StdError + Send + Sync> = Error::other("job failed").into();
        assert_eq!(boxed.to_string(), "job failed");
    }
}
