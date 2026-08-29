// SPDX-License-Identifier: Apache-2.0
//! Fixed-capacity session storage with explicit admission pressure.

/// A bounded pool that never replaces an occupied slot.
pub struct BoundedSessionPool<T, const N: usize> {
    slots: [Option<T>; N],
}

impl<T, const N: usize> BoundedSessionPool<T, N> {
    /// Create an empty pool.
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; N],
        }
    }

    /// Insert into the first empty slot.
    ///
    /// Returns the slot index, or returns `item` unchanged when the pool is full.
    pub fn try_insert(&mut self, item: T) -> Result<usize, T> {
        let Some(slot) = self.slots.iter().position(Option::is_none) else {
            return Err(item);
        };
        self.slots[slot] = Some(item);
        Ok(slot)
    }

    /// Borrow one occupied slot mutably.
    pub fn get_mut(&mut self, slot: usize) -> Option<&mut T> {
        self.slots.get_mut(slot)?.as_mut()
    }

    /// Remove every item matching `predicate`.
    pub fn remove_where(&mut self, mut predicate: impl FnMut(&T) -> bool) {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(&mut predicate) {
                *slot = None;
            }
        }
    }

    /// Return the number of occupied slots.
    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// Return whether the pool has no occupied slots.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return whether another item can be admitted without displacement.
    pub fn is_full(&self) -> bool {
        self.len() == N
    }
}

impl<T, const N: usize> Default for BoundedSessionPool<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_pool_returns_pressure_without_displacement() {
        let mut pool = BoundedSessionPool::<u32, 4>::new();
        for value in 10..14 {
            assert_eq!(pool.try_insert(value), Ok((value - 10) as usize));
        }
        assert!(pool.is_full());
        assert_eq!(pool.try_insert(99), Err(99));
        for slot in 0..4 {
            assert_eq!(pool.get_mut(slot).copied(), Some(10 + slot as u32));
        }
    }

    #[test]
    fn removal_opens_one_slot_without_touching_survivors() {
        let mut pool = BoundedSessionPool::<u32, 4>::new();
        for value in 10..14 {
            pool.try_insert(value).unwrap();
        }
        pool.remove_where(|value| *value == 11);
        assert_eq!(pool.len(), 3);
        assert_eq!(pool.try_insert(20), Ok(1));
        assert_eq!(pool.get_mut(0).copied(), Some(10));
        assert_eq!(pool.get_mut(1).copied(), Some(20));
        assert_eq!(pool.get_mut(2).copied(), Some(12));
        assert_eq!(pool.get_mut(3).copied(), Some(13));
    }
}
