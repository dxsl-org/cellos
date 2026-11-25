// SPDX-License-Identifier: MIT
//! External-interrupt vectors a VMM has raised for a vCPU and the guest has
//! not taken yet.
//!
//! A set, like the request register of an interrupt controller: raising a
//! vector that already waits is still one interrupt. The backends inject from
//! it when the guest can take an interrupt, so nothing raised is dropped.

#[derive(Clone, Copy, Default)]
pub struct PendingIrqs {
    vectors: [u64; 4],
}

impl PendingIrqs {
    pub fn raise(&mut self, vector: u8) {
        self.vectors[usize::from(vector >> 6)] |= 1 << (vector & 63);
    }

    pub fn clear(&mut self, vector: u8) {
        self.vectors[usize::from(vector >> 6)] &= !(1 << (vector & 63));
    }

    /// The lowest waiting vector whose priority class (its high four bits) is
    /// above `tpr`, the guest's task priority: lower classes are held back, as
    /// the CPU would hold them. With `tpr` 0 that still excludes vectors 0-15,
    /// which are not external interrupts.
    ///
    /// Lowest first because the VMM's legacy interrupt controller gives its
    /// highest-priority line (the timer) the lowest vector of its block.
    pub fn lowest_above(&self, tpr: u8) -> Option<u8> {
        let first = (usize::from(tpr) + 1) << 4;
        (first / 64..self.vectors.len()).find_map(|word| {
            let below = if word == first / 64 {
                (1u64 << (first % 64)) - 1
            } else {
                0
            };
            let waiting = self.vectors[word] & !below;
            (waiting != 0).then(|| (word * 64) as u8 + waiting.trailing_zeros() as u8)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lowest_waiting_vector_goes_first_and_once() {
        let mut pending = PendingIrqs::default();
        for vector in [0xEF, 0x33, 0x30, 0x33, 0x80] {
            pending.raise(vector);
        }
        let mut taken = [0u8; 5];
        let mut count = 0;
        while let Some(vector) = pending.lowest_above(0) {
            pending.clear(vector);
            taken[count] = vector;
            count += 1;
        }
        assert_eq!(&taken[..count], &[0x30, 0x33, 0x80, 0xEF]);
    }

    #[test]
    fn vectors_at_or_below_the_task_priority_class_wait() {
        let mut pending = PendingIrqs::default();
        pending.raise(0x0F); // not an external-interrupt vector
        pending.raise(0x3F);
        pending.raise(0x40);
        assert_eq!(pending.lowest_above(0), Some(0x3F));
        assert_eq!(pending.lowest_above(2), Some(0x3F));
        assert_eq!(pending.lowest_above(3), Some(0x40));
        assert_eq!(pending.lowest_above(4), None);
        assert_eq!(pending.lowest_above(15), None);
        pending.raise(0xFF);
        assert_eq!(pending.lowest_above(14), Some(0xFF));
    }
}
