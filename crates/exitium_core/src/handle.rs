//! Generational handles: small, copyable ids that detect stale references.
//!
//! A handle names a slot by its `index` and the value in that slot by its
//! `generation`. Removing a value bumps its slot's generation, so every
//! handle to the removed value stops matching: a lookup with it returns
//! `None` and never reaches a value that later reuses the slot.
//! [`Slab`](crate::Slab) issues and checks handles. [`RawHandle`] is also the
//! layout for ids that other crates allocate themselves, such as the ECS
//! `Entity`.
//!
//! # Layout
//!
//! A handle is 8 bytes: a `u32` index and a [`NonZeroU32`] generation.
//! Generations start at 1 and no handle has generation 0, so the compiler
//! uses 0 to mean `None`: `Option<RawHandle>` and `Option<Handle<T>>` are 8
//! bytes too. Bits from an untrusted source go through the fallible
//! [`RawHandle::from_bits`].
//!
//! A handle is plain data, not a reference count. Holding one keeps nothing
//! alive, and building one from raw parts is safe: the slab checks on every
//! lookup whether it still matches.
//!
//! ```
//! use std::num::NonZeroU32;
//! use exitium_core::RawHandle;
//!
//! let handle = RawHandle::new(7, NonZeroU32::MIN);
//! assert_eq!(RawHandle::from_bits(handle.to_bits()), Some(handle));
//! assert_eq!(RawHandle::from_bits(7 << 32), None); // generation 0
//! assert_eq!(size_of::<Option<RawHandle>>(), 8);
//! ```

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::num::NonZeroU32;

/// An untyped generational handle: a slot index and the generation of the
/// value in that slot.
///
/// [`Handle<T>`] is the typed form that [`Slab<T>`](crate::Slab) hands out.
/// Use `RawHandle` where the value type is not known statically, such as in
/// the ECS `Entity` id or in type-erased tables. Ordered by index, then
/// generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RawHandle {
    /// The slot index.
    pub index: u32,
    /// The generation of the value in the slot. Generations start at 1.
    pub generation: NonZeroU32,
}

impl RawHandle {
    /// A handle to slot `index` at `generation`.
    ///
    /// To build one from an untrusted `u32` generation, check it with
    /// [`NonZeroU32::new`] first.
    #[must_use]
    #[inline]
    pub const fn new(index: u32, generation: NonZeroU32) -> Self {
        Self { index, generation }
    }

    /// Packs the handle into a `u64`: the index in the high 32 bits and the
    /// generation in the low 32. Comparing packed values orders handles the
    /// same way as [`Ord`].
    #[must_use]
    #[inline]
    pub const fn to_bits(self) -> u64 {
        // Widening casts, because `u64::from` is not `const`.
        ((self.index as u64) << 32) | self.generation.get() as u64
    }

    /// Unpacks a value made by [`to_bits`](Self::to_bits). Returns `None` if
    /// the low 32 bits are 0, because no handle has generation 0.
    #[must_use]
    #[inline]
    pub const fn from_bits(bits: u64) -> Option<Self> {
        // Each cast keeps exactly the 32 bits it names.
        let index = (bits >> 32) as u32;
        match NonZeroU32::new(bits as u32) {
            Some(generation) => Some(Self::new(index, generation)),
            None => None,
        }
    }
}

impl Hash for RawHandle {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        // One `u64` write instead of two `u32` writes. Equal handles have
        // equal bits, so this agrees with `==`.
        self.to_bits().hash(state);
    }
}

/// A typed generational handle to a `T` stored in a [`Slab<T>`](crate::Slab).
///
/// The same 8 bytes as [`RawHandle`]: `T` exists only at compile time, so a
/// handle to a texture cannot be passed where a handle to a sound is
/// expected. `Handle<T>` is `Copy`, `Eq`, `Ord`, `Hash`, `Debug`, `Send` and
/// `Sync` whatever `T` is. Ordered by index, then generation.
///
/// A handle does not record which slab issued it. Used with another slab of
/// the same type, it finds whatever that slab holds at the same index and
/// generation, if anything.
pub struct Handle<T> {
    raw: RawHandle,
    // `fn() -> T` rather than `T`: a handle does not own a `T`, so it stays
    // `Send` and `Sync` when `T` is not.
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    /// Gives `raw` the value type `T`, for example to restore a handle that
    /// was stored as a [`RawHandle`].
    #[must_use]
    #[inline]
    pub const fn from_raw(raw: RawHandle) -> Self {
        Self {
            raw,
            marker: PhantomData,
        }
    }

    /// The untyped handle.
    #[must_use]
    #[inline]
    pub const fn raw(self) -> RawHandle {
        self.raw
    }

    /// The slot index.
    #[must_use]
    #[inline]
    pub const fn index(self) -> u32 {
        self.raw.index
    }

    /// The generation of the value this handle names.
    #[must_use]
    #[inline]
    pub const fn generation(self) -> NonZeroU32 {
        self.raw.generation
    }
}

impl<T> From<Handle<T>> for RawHandle {
    #[inline]
    fn from(handle: Handle<T>) -> Self {
        handle.raw
    }
}

// The trait impls below are written by hand because `derive` would require
// `T: Clone`, `T: PartialEq` and so on, but a handle is copyable and
// comparable whatever `T` is.

impl<T> Clone for Handle<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for Handle<T> {}

impl<T> PartialOrd for Handle<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Handle<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl<T> Hash for Handle<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handle")
            .field("index", &self.raw.index)
            .field("generation", &self.raw.generation)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::hash::DefaultHasher;
    use std::rc::Rc;

    const MAX: u32 = u32::MAX;

    fn raw(index: u32, generation: u32) -> RawHandle {
        RawHandle::new(index, NonZeroU32::new(generation).unwrap())
    }

    fn hash_of(value: impl Hash) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    /// Distinct handles in ascending order, covering both ends of the index
    /// and generation ranges.
    fn ascending() -> [RawHandle; 7] {
        [
            raw(0, 1),
            raw(0, 2),
            raw(0, MAX),
            raw(1, 1),
            raw(1, MAX),
            raw(MAX, 1),
            raw(MAX, MAX),
        ]
    }

    /// A type with no trait impls at all.
    struct Opaque;

    #[test]
    fn handles_are_8_bytes_and_none_is_free() {
        assert_eq!(size_of::<RawHandle>(), 8);
        assert_eq!(size_of::<Option<RawHandle>>(), 8);
        assert_eq!(size_of::<Handle<u8>>(), 8);
        assert_eq!(size_of::<Handle<[u64; 64]>>(), 8);
        assert_eq!(size_of::<Handle<Opaque>>(), 8);
        assert_eq!(size_of::<Option<Handle<String>>>(), 8);
    }

    #[test]
    fn bits_round_trip_with_index_high_and_generation_low() {
        for handle in ascending() {
            let bits = handle.to_bits();
            assert_eq!(
                bits,
                (u64::from(handle.index) << 32) | u64::from(handle.generation.get())
            );
            assert_eq!(RawHandle::from_bits(bits), Some(handle));
        }
    }

    #[test]
    fn from_bits_rejects_generation_zero() {
        for index in [0, 1, 7, MAX] {
            assert_eq!(
                RawHandle::from_bits(u64::from(index) << 32),
                None,
                "{index}"
            );
        }
    }

    #[test]
    fn ordering_is_by_index_then_generation_and_matches_the_bits() {
        let handles = ascending();
        for (i, a) in handles.iter().enumerate() {
            for (j, b) in handles.iter().enumerate() {
                let expected = i.cmp(&j);
                assert_eq!(a.cmp(b), expected, "{a:?} vs {b:?}");
                assert_eq!(a.partial_cmp(b), Some(expected));
                assert_eq!(a.to_bits().cmp(&b.to_bits()), expected);
                assert_eq!(a == b, i == j);

                let (a, b) = (Handle::<Opaque>::from_raw(*a), Handle::from_raw(*b));
                assert_eq!(a.cmp(&b), expected);
                assert_eq!(a.partial_cmp(&b), Some(expected));
                assert_eq!(a == b, i == j);
            }
        }
    }

    #[test]
    fn hash_agrees_with_eq() {
        let with_copies = ascending().into_iter().chain(ascending());
        assert_eq!(with_copies.collect::<HashSet<_>>().len(), ascending().len());
        // The hash tells index and generation apart, not just the two numbers.
        assert_ne!(hash_of(raw(1, 2)), hash_of(raw(2, 1)));
    }

    #[test]
    fn typed_handles_need_no_bounds_on_t() {
        fn assert_send_sync<T: Send + Sync>() {}
        // `Rc` is neither `Send` nor `Sync`; a handle to one is both.
        assert_send_sync::<Handle<Rc<()>>>();

        let handle = Handle::<Opaque>::from_raw(raw(3, 1));
        let copy = handle;
        assert_eq!(handle, copy);
        assert_eq!(
            HashSet::from([handle, copy, Handle::from_raw(raw(4, 1))]).len(),
            2
        );
        assert_eq!(format!("{handle:?}"), "Handle { index: 3, generation: 1 }");
    }

    #[test]
    fn typed_and_raw_conversions_agree() {
        for handle in ascending() {
            let typed = Handle::<Opaque>::from_raw(handle);
            assert_eq!(typed.raw(), handle);
            assert_eq!(RawHandle::from(typed), handle);
            assert_eq!(typed.index(), handle.index);
            assert_eq!(typed.generation(), handle.generation);
        }
        assert_eq!(
            RawHandle::new(5, NonZeroU32::MIN),
            RawHandle {
                index: 5,
                generation: NonZeroU32::MIN
            }
        );
    }
}
