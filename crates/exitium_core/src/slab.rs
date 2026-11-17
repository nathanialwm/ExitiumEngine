//! [`Slab<T>`]: values stored in a vector of slots and addressed by
//! [`Handle<T>`]s.
//!
//! [`insert`](Slab::insert) puts a value in a free slot and returns a handle
//! naming the slot and its current generation. [`get`](Slab::get),
//! [`get_mut`](Slab::get_mut), [`contains`](Slab::contains) and
//! [`remove`](Slab::remove) are O(1): they index the slot vector and compare
//! generations, with no hashing. Values are stored inline in one `Vec`, so
//! there is no allocation per value.
//!
//! # Slot reuse and stale handles
//!
//! Removing a value frees its slot and bumps the slot's generation. The next
//! insert reuses the most recently freed slot (last in, first out, which
//! favours memory that is still in cache), and its handle carries the new
//! generation. Handles to the removed value keep the old generation: they are
//! *stale*, and a lookup with one returns `None`. Within one slab, a handle
//! only ever names the value it was issued for.
//!
//! # Generation overflow
//!
//! Generations are [`NonZeroU32`]s starting at 1, so one slot can hold
//! 2^32 - 1 values over its life. When the value with the last generation
//! (`u32::MAX`) is removed, the slot is *retired* instead of wrapping back to
//! generation 1, which would let the oldest handles match again. A retired
//! slot is never reused and keeps its memory until the slab is dropped;
//! [`Slab::retired`] counts them. Even a slot reused a million times a second
//! lasts over an hour, so retired slots waste little memory.
//!
//! # Iteration order
//!
//! [`Slab::iter`] and [`Slab::iter_mut`] visit values in ascending slot
//! index. That is insertion order only until a slot is reused, but it is
//! deterministic: the same inserts and removes always give the same order.

use std::fmt;
use std::iter::{Enumerate, FusedIterator};
use std::mem;
use std::num::NonZeroU32;
use std::slice;

use crate::handle::{Handle, RawHandle};

/// A collection of `T` values addressed by [`Handle<T>`]s.
///
/// See the [module docs](crate::slab) for how slots are reused, what happens
/// when a slot runs out of generations, and the iteration order.
///
/// ```
/// use exitium_core::Slab;
///
/// let mut items = Slab::new();
/// let sword = items.insert("sword");
/// let shield = items.insert("shield");
/// assert_eq!(items.get(sword), Some(&"sword"));
///
/// assert_eq!(items.remove(sword), Some("sword"));
/// assert_eq!(items.get(sword), None); // stale: the sword is gone
///
/// // The bow reuses the sword's slot with a new generation, so the sword's
/// // handle still finds nothing.
/// let bow = items.insert("bow");
/// assert_eq!(bow.index(), sword.index());
/// assert_eq!(items.get(sword), None);
/// assert_eq!(items.get(bow), Some(&"bow"));
///
/// // Slot order, not insertion order.
/// let all: Vec<_> = items.iter().collect();
/// assert_eq!(all, [(bow, &"bow"), (shield, &"shield")]);
/// ```
// Invariants:
// - `len` counts the `Occupied` slots and `retired` the `Retired` ones.
// - `free_head` and the `next_free` links chain every `Vacant` slot exactly
//   once.
// - `slots.len() <= 2^32`, so every slot position fits in a `u32` index.
// - A slot's generation only grows, so no handle is issued twice.
#[derive(Clone)]
pub struct Slab<T> {
    slots: Vec<Slot<T>>,
    free_head: Option<u32>,
    len: usize,
    retired: usize,
}

#[derive(Clone)]
enum Slot<T> {
    /// Holds a value. Handles with this generation name it.
    Occupied { generation: NonZeroU32, value: T },
    /// On the free list. The next value stored here gets `generation`.
    Vacant {
        generation: NonZeroU32,
        next_free: Option<u32>,
    },
    /// Freed at generation `u32::MAX`, so never reused.
    Retired,
}

impl<T> Slab<T> {
    /// An empty slab. It does not allocate until the first insert.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_head: None,
            len: 0,
            retired: 0,
        }
    }

    /// An empty slab with room for `capacity` values before it reallocates.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            ..Self::new()
        }
    }

    /// The number of values in the slab.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// `true` if the slab holds no values.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The number of retired slots: slots that ran out of generations and
    /// are never reused. See [generation overflow](crate::slab#generation-overflow).
    #[must_use]
    #[inline]
    pub const fn retired(&self) -> usize {
        self.retired
    }

    /// Stores `value` and returns a handle to it.
    ///
    /// Reuses the most recently freed slot, or appends a new slot when none
    /// is free.
    ///
    /// # Panics
    ///
    /// If all 2^32 slot indices are in use, counting free and retired slots.
    /// The slots alone would take tens of gigabytes, so in practice memory
    /// runs out first.
    pub fn insert(&mut self, value: T) -> Handle<T> {
        let raw = match self.free_head {
            Some(index) => {
                let slot = &mut self.slots[index as usize];
                let Slot::Vacant {
                    generation,
                    next_free,
                } = *slot
                else {
                    unreachable!("the free list links only vacant slots");
                };
                self.free_head = next_free;
                *slot = Slot::Occupied { generation, value };
                RawHandle::new(index, generation)
            }
            None => {
                let index = u32::try_from(self.slots.len())
                    .expect("Slab::insert: all 2^32 slot indices are in use");
                let generation = NonZeroU32::MIN;
                self.slots.push(Slot::Occupied { generation, value });
                RawHandle::new(index, generation)
            }
        };
        self.len += 1;
        Handle::from_raw(raw)
    }

    /// The value `handle` names, or `None` if the handle is stale.
    #[must_use]
    #[inline]
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        match self.slots.get(handle.index() as usize)? {
            Slot::Occupied { generation, value } if *generation == handle.generation() => {
                Some(value)
            }
            _ => None,
        }
    }

    /// The value `handle` names, mutably, or `None` if the handle is stale.
    #[must_use]
    #[inline]
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        match self.slots.get_mut(handle.index() as usize)? {
            Slot::Occupied { generation, value } if *generation == handle.generation() => {
                Some(value)
            }
            _ => None,
        }
    }

    /// `true` if `handle` names a value in the slab.
    #[must_use]
    #[inline]
    pub fn contains(&self, handle: Handle<T>) -> bool {
        self.get(handle).is_some()
    }

    /// Removes and returns the value `handle` names, or returns `None` if the
    /// handle is already stale.
    ///
    /// `handle` and all its copies go stale. The slot is reused by a later
    /// insert, or retired if this was its last generation.
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        if self.contains(handle) {
            self.vacate(handle.index())
        } else {
            None
        }
    }

    /// Removes every value, as if each were [`remove`](Self::remove)d.
    ///
    /// Every handle goes stale and stays stale: generations are bumped, not
    /// reset, and slots at their last generation are retired. Later inserts
    /// reuse the freed slots, lowest index first, before any slot that was
    /// already free.
    ///
    /// Values are dropped one at a time, each after its slot is freed. If a
    /// destructor panics, the values not yet dropped stay in the slab, and
    /// the slab remains usable.
    pub fn clear(&mut self) {
        // Highest slot first, so the lowest freed slot ends up at the head of
        // the free list.
        for position in (0..self.slots.len()).rev() {
            drop(self.vacate(slot_index(position)));
        }
    }

    /// An iterator over `(handle, &value)` pairs, in ascending slot index.
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            slots: self.slots.iter().enumerate(),
            remaining: self.len,
        }
    }

    /// An iterator over `(handle, &mut value)` pairs, in ascending slot
    /// index.
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            slots: self.slots.iter_mut().enumerate(),
            remaining: self.len,
        }
    }

    /// Takes the value out of slot `index`, if the slot holds one, and frees
    /// the slot: its generation is bumped and it goes on the free list, or it
    /// is retired if that was its last generation. The bookkeeping is done
    /// before the value is returned, so the slab is consistent when the
    /// caller drops it.
    fn vacate(&mut self, index: u32) -> Option<T> {
        let slot = &mut self.slots[index as usize];
        let Slot::Occupied { generation, .. } = *slot else {
            return None;
        };
        let freed = match generation.checked_add(1) {
            Some(next_generation) => {
                // The freed slot becomes the head of the free list.
                let next_free = self.free_head.replace(index);
                Slot::Vacant {
                    generation: next_generation,
                    next_free,
                }
            }
            None => {
                self.retired += 1;
                Slot::Retired
            }
        };
        self.len -= 1;
        match mem::replace(slot, freed) {
            Slot::Occupied { value, .. } => Some(value),
            Slot::Vacant { .. } | Slot::Retired => unreachable!("the slot was occupied"),
        }
    }
}

impl<T> Default for Slab<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: fmt::Debug> fmt::Debug for Slab<T> {
    /// Formats the slab as a map from handle to value, in slot order.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'a, T> IntoIterator for &'a Slab<T> {
    type Item = (Handle<T>, &'a T);
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut Slab<T> {
    type Item = (Handle<T>, &'a mut T);
    type IntoIter = IterMut<'a, T>;

    fn into_iter(self) -> IterMut<'a, T> {
        self.iter_mut()
    }
}

/// An iterator over `(handle, &value)` pairs in ascending slot index, made
/// by [`Slab::iter`].
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct Iter<'a, T> {
    slots: Enumerate<slice::Iter<'a, Slot<T>>>,
    // Values not yet yielded. Makes `size_hint` exact and skips scanning
    // trailing free slots.
    remaining: usize,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (Handle<T>, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let item = self.slots.find_map(|(position, slot)| match slot {
            Slot::Occupied { generation, value } => Some((handle_at(position, *generation), value)),
            _ => None,
        })?;
        self.remaining -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}

impl<T> FusedIterator for Iter<'_, T> {}

/// An iterator over `(handle, &mut value)` pairs in ascending slot index,
/// made by [`Slab::iter_mut`].
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct IterMut<'a, T> {
    slots: Enumerate<slice::IterMut<'a, Slot<T>>>,
    // Same role as in `Iter`.
    remaining: usize,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = (Handle<T>, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let item = self.slots.find_map(|(position, slot)| match slot {
            Slot::Occupied { generation, value } => Some((handle_at(position, *generation), value)),
            _ => None,
        })?;
        self.remaining -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<T> ExactSizeIterator for IterMut<'_, T> {}

impl<T> FusedIterator for IterMut<'_, T> {}

/// The handle for the value at slot `position` with `generation`.
fn handle_at<T>(position: usize, generation: NonZeroU32) -> Handle<T> {
    Handle::from_raw(RawHandle::new(slot_index(position), generation))
}

/// Converts a slot position to its `u32` index. It never truncates: `insert`
/// stops at 2^32 slots.
fn slot_index(position: usize) -> u32 {
    debug_assert!(
        u32::try_from(position).is_ok(),
        "slot {position} is past u32"
    );
    position as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{self, AssertUnwindSafe};
    use std::rc::Rc;

    fn handle<T>(index: u32, generation: u32) -> Handle<T> {
        Handle::from_raw(RawHandle::new(index, NonZeroU32::new(generation).unwrap()))
    }

    /// Moves slot `index` to `generation` in place. Stands in for the
    /// billions of remove and insert cycles it would take to get there.
    fn set_generation<T>(slab: &mut Slab<T>, index: u32, to: u32) {
        match &mut slab.slots[index as usize] {
            Slot::Occupied { generation, .. } | Slot::Vacant { generation, .. } => {
                *generation = NonZeroU32::new(to).unwrap();
            }
            Slot::Retired => panic!("slot {index} is retired"),
        }
    }

    /// Panics unless the bookkeeping matches the slots (the invariants on
    /// `Slab`).
    fn assert_consistent<T>(slab: &Slab<T>) {
        let count = |wanted: fn(&Slot<T>) -> bool| slab.slots.iter().filter(|s| wanted(s)).count();
        let occupied = count(|slot| matches!(slot, Slot::Occupied { .. }));
        let retired = count(|slot| matches!(slot, Slot::Retired));
        let vacant = count(|slot| matches!(slot, Slot::Vacant { .. }));
        assert_eq!(slab.len(), occupied, "len");
        assert_eq!(slab.retired(), retired, "retired");

        let mut linked = vec![false; slab.slots.len()];
        let mut next = slab.free_head;
        while let Some(index) = next {
            let Slot::Vacant { next_free, .. } = slab.slots[index as usize] else {
                panic!("the free list links slot {index}, which is not vacant");
            };
            assert!(!linked[index as usize], "slot {index} is linked twice");
            linked[index as usize] = true;
            next = next_free;
        }
        assert_eq!(
            linked.iter().filter(|&&is_linked| is_linked).count(),
            vacant
        );
        assert_eq!(slab.iter().count(), occupied);
    }

    #[test]
    fn new_and_default_slabs_are_empty() {
        let slabs = [Slab::<u8>::new(), Slab::default(), Slab::with_capacity(8)];
        for slab in &slabs {
            assert!(slab.is_empty());
            assert_eq!(slab.len(), 0);
            assert_eq!(slab.retired(), 0);
            assert_eq!(slab.iter().next(), None);
            assert!(!slab.contains(handle(0, 1)));
            assert_consistent(slab);
        }
        assert!(slabs[2].slots.capacity() >= 8);
    }

    #[test]
    fn insert_then_get_and_get_mut() {
        let mut slab = Slab::new();
        let a = slab.insert(String::from("a"));
        let b = slab.insert(String::from("b"));
        assert_eq!(a, handle(0, 1));
        assert_eq!(b, handle(1, 1));
        assert_eq!(slab.len(), 2);
        assert!(!slab.is_empty());
        assert!(slab.contains(a) && slab.contains(b));

        slab.get_mut(b).unwrap().push('!');
        assert_eq!(slab.get(a).map(String::as_str), Some("a"));
        assert_eq!(slab.get(b).map(String::as_str), Some("b!"));
        assert_consistent(&slab);
    }

    #[test]
    fn remove_leaves_the_handle_stale() {
        let mut slab = Slab::new();
        let a = slab.insert(10);
        let b = slab.insert(20);
        assert_eq!(slab.remove(a), Some(10));
        assert_eq!(slab.len(), 1);
        assert_eq!(slab.get(a), None);
        assert_eq!(slab.get_mut(a), None);
        assert!(!slab.contains(a));
        assert_eq!(slab.remove(a), None);
        assert_eq!(slab.len(), 1);
        assert_eq!(slab.get(b), Some(&20));
        assert_consistent(&slab);
    }

    #[test]
    fn freed_slots_are_reused_last_in_first_out_with_the_next_generation() {
        let mut slab = Slab::new();
        let a = slab.insert('a');
        let b = slab.insert('b');
        let c = slab.insert('c');
        slab.remove(a);
        slab.remove(c);

        let d = slab.insert('d');
        let e = slab.insert('e');
        let f = slab.insert('f');
        assert_eq!(d, handle(2, 2), "c's slot, freed last, is reused first");
        assert_eq!(e, handle(0, 2));
        assert_eq!(f, handle(3, 1), "no free slot left, so the slab grows");

        assert_eq!((slab.get(a), slab.get(c)), (None, None));
        for (live, value) in [(b, 'b'), (d, 'd'), (e, 'e'), (f, 'f')] {
            assert_eq!(slab.get(live), Some(&value));
        }
        assert_consistent(&slab);
    }

    #[test]
    fn stale_handles_never_reach_later_occupants_of_their_slot() {
        let mut slab = Slab::new();
        let mut stale = Vec::new();
        let mut current = slab.insert(0);
        for value in 1..100 {
            assert_eq!(slab.remove(current), Some(value - 1));
            stale.push(current);
            current = slab.insert(value);
            assert_eq!(current, handle(0, value + 1));
        }
        for old in stale {
            assert_eq!(slab.get(old), None);
            assert_eq!(slab.get_mut(old), None);
            assert_eq!(slab.remove(old), None, "must not remove the current value");
        }
        assert_eq!(slab.get(current), Some(&99));
        assert_consistent(&slab);
    }

    #[test]
    fn handles_never_issued_match_nothing() {
        let mut slab = Slab::new();
        let a = slab.insert('a');
        let b = slab.insert('b');
        slab.remove(b);
        let never_issued = [
            handle(1, 2),        // the free slot's next generation
            handle(0, 2),        // a generation slot 0 has not reached
            handle(2, 1),        // past the last slot
            handle(u32::MAX, 1), // far past it
        ];
        for forged in never_issued {
            assert!(!slab.contains(forged), "{forged:?}");
            assert_eq!(slab.get_mut(forged), None);
            assert_eq!(slab.remove(forged), None);
        }
        assert_eq!(slab.get(a), Some(&'a'));
        assert_consistent(&slab);
    }

    #[test]
    fn iteration_is_in_slot_order_not_insertion_order() {
        let mut slab = Slab::new();
        let a = slab.insert('a');
        let b = slab.insert('b');
        let c = slab.insert('c');
        slab.remove(a);

        let mut iter = slab.iter();
        assert_eq!(iter.len(), 2);
        assert_eq!(iter.next(), Some((b, &'b')));
        assert_eq!(iter.len(), 1);
        assert_eq!(iter.next(), Some((c, &'c')));
        assert_eq!(iter.next(), None);

        // The newest value takes the free slot 0, so it comes first.
        let d = slab.insert('d');
        let in_order: Vec<_> = (&slab).into_iter().collect();
        assert_eq!(in_order, [(d, &'d'), (b, &'b'), (c, &'c')]);
    }

    #[test]
    fn iteration_stops_before_trailing_free_slots() {
        let mut slab = Slab::new();
        let a = slab.insert(1);
        let tail: Vec<_> = (2..6).map(|value| slab.insert(value)).collect();
        for old in tail {
            slab.remove(old);
        }
        let mut iter = slab.iter();
        assert_eq!(iter.next(), Some((a, &1)));
        assert_eq!(iter.len(), 0);
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);
        // `remaining` hit zero, so the trailing free slots were never read.
        assert_eq!(iter.slots.len(), 4);
    }

    #[test]
    fn iter_mut_updates_values_in_place() {
        let mut slab = Slab::new();
        let a = slab.insert(1);
        let b = slab.insert(2);
        let c = slab.insert(3);
        slab.remove(b);

        let mut iter = slab.iter_mut();
        assert_eq!(iter.len(), 2);
        for (_, value) in iter.by_ref() {
            *value *= 10;
        }
        assert_eq!(iter.next(), None);
        for (id, value) in &mut slab {
            if id == c {
                *value += 5;
            }
        }
        let values: Vec<_> = slab.iter().map(|(h, v)| (h, *v)).collect();
        assert_eq!(values, [(a, 10), (c, 35)]);
    }

    #[test]
    fn the_last_generation_retires_the_slot_instead_of_wrapping() {
        let mut slab = Slab::new();
        let first = slab.insert('a');
        set_generation(&mut slab, 0, u32::MAX - 1);
        let second_to_last = handle(0, u32::MAX - 1);

        // One generation is left, so the slot is still reused.
        assert_eq!(slab.remove(second_to_last), Some('a'));
        let last = slab.insert('b');
        assert_eq!(last, handle(0, u32::MAX));
        assert_eq!(slab.retired(), 0);

        // No generation is left: the slot retires rather than wrapping to 1.
        assert_eq!(slab.remove(last), Some('b'));
        assert_eq!(slab.retired(), 1);
        assert!(slab.is_empty());
        assert_consistent(&slab);

        let fresh = slab.insert('c');
        assert_eq!(fresh, handle(1, 1), "the retired slot is never reused");
        for old in [first, second_to_last, last] {
            assert!(!slab.contains(old), "{old:?}");
            assert_eq!(slab.remove(old), None);
        }
        assert_eq!(slab.iter().collect::<Vec<_>>(), [(fresh, &'c')]);
        assert_consistent(&slab);
    }

    #[test]
    fn clear_keeps_old_handles_stale_and_reuses_slots_lowest_first() {
        let mut slab = Slab::new();
        let old: Vec<_> = (0..4).map(|value| slab.insert(value)).collect();
        slab.remove(old[1]); // already free before the clear
        slab.clear();
        assert!(slab.is_empty());
        assert_eq!(slab.iter().next(), None);
        assert_consistent(&slab);

        let new: Vec<_> = (10..15).map(|value| slab.insert(value)).collect();
        assert_eq!(
            new,
            [
                handle(0, 2),
                handle(2, 2),
                handle(3, 2),
                handle(1, 2), // the slot that was already free comes last
                handle(4, 1), // only then does the slab grow
            ]
        );
        for stale in old {
            assert!(!slab.contains(stale), "{stale:?}");
        }
        assert_consistent(&slab);
    }

    #[test]
    fn clear_retires_slots_at_their_last_generation() {
        let mut slab = Slab::new();
        slab.insert('a');
        slab.insert('b');
        set_generation(&mut slab, 1, u32::MAX);
        slab.clear();
        assert_eq!(slab.retired(), 1);
        assert_consistent(&slab);
        assert_eq!(slab.insert('c'), handle(0, 2));
        assert_eq!(slab.insert('d'), handle(2, 1));
    }

    #[test]
    fn values_are_dropped_exactly_once() {
        let tracker = Rc::new(());
        let live = || Rc::strong_count(&tracker) - 1;
        let mut slab = Slab::new();
        let a = slab.insert(Rc::clone(&tracker));
        slab.insert(Rc::clone(&tracker));
        slab.insert(Rc::clone(&tracker));
        assert_eq!(live(), 3);

        let removed = slab.remove(a);
        assert_eq!(live(), 3, "remove hands the value back without dropping it");
        drop(removed);
        assert_eq!(live(), 2);

        slab.clear();
        assert_eq!(live(), 0);

        slab.insert(Rc::clone(&tracker));
        drop(slab);
        assert_eq!(live(), 0);
    }

    #[test]
    fn a_panicking_destructor_leaves_clear_consistent() {
        struct PanicOnDrop(bool);
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                assert!(!self.0, "destructor panic");
            }
        }

        let mut slab = Slab::new();
        let low = slab.insert(PanicOnDrop(false));
        let middle = slab.insert(PanicOnDrop(true));
        let high = slab.insert(PanicOnDrop(false));
        let result = panic::catch_unwind(AssertUnwindSafe(|| slab.clear()));
        assert!(result.is_err());

        // `clear` works down from the highest slot and stopped at the
        // panicking value: that slot and the ones above are freed, the ones
        // below are untouched.
        assert!(!slab.contains(high) && !slab.contains(middle));
        assert!(slab.contains(low));
        assert_eq!(slab.len(), 1);
        assert_consistent(&slab);

        slab.clear();
        assert!(slab.is_empty());
        assert_consistent(&slab);
    }

    #[test]
    fn clones_are_independent_and_accept_the_same_handles() {
        let mut slab = Slab::new();
        let a = slab.insert(1);
        let mut copy = slab.clone();
        *copy.get_mut(a).unwrap() = 2;
        assert_eq!((slab.get(a), copy.get(a)), (Some(&1), Some(&2)));
        assert_eq!(copy.remove(a), Some(2));
        assert_eq!(slab.get(a), Some(&1));
    }

    #[test]
    fn debug_formats_live_values_by_handle() {
        let mut slab = Slab::new();
        slab.insert('a');
        let b = slab.insert('b');
        slab.remove(b);
        slab.insert('c');
        assert_eq!(
            format!("{slab:?}"),
            "{Handle { index: 0, generation: 1 }: 'a', Handle { index: 1, generation: 2 }: 'c'}"
        );
    }
}
