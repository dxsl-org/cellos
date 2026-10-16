//! Bounded queue management and deadline eviction.

use alloc::collections::VecDeque;

pub struct QueuedItem {
    pub request_id: u64,
    pub deadline_ms: u64,
    pub payload_len: usize,
}

pub struct BoundedQueue {
    items: VecDeque<QueuedItem>,
    max_items: usize,
    max_bytes: usize,
    current_bytes: usize,
}

impl BoundedQueue {
    pub fn new(max_items: usize, max_bytes: usize) -> Self {
        Self {
            items: VecDeque::new(),
            max_items,
            max_bytes,
            current_bytes: 0,
        }
    }

    /// Evict all items whose deadline has passed.
    pub fn evict_expired(&mut self, now_ms: u64) -> usize {
        let before_count = self.items.len();
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].deadline_ms <= now_ms {
                let item = self.items.remove(i).unwrap();
                self.current_bytes = self.current_bytes.saturating_sub(item.payload_len);
            } else {
                i += 1;
            }
        }
        before_count - self.items.len()
    }

    /// Push an item if capacity allows.
    pub fn push(&mut self, item: QueuedItem) -> Result<(), ()> {
        if self.items.len() >= self.max_items
            || self.current_bytes.saturating_add(item.payload_len) > self.max_bytes
        {
            return Err(());
        }
        self.current_bytes += item.payload_len;
        self.items.push_back(item);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
