//! Row buffering for streaming with head/tail retention.

use crate::types::CellValue;

/// A ring buffer that maintains the last N items.
#[derive(Debug)]
pub struct RingBuffer<T> {
    buffer: Vec<Option<T>>,
    capacity: usize,
    write_pos: usize,
    count: usize,
}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        let mut buffer = Vec::with_capacity(capacity);
        buffer.resize_with(capacity, || None);
        Self {
            buffer,
            capacity,
            write_pos: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, item: T) {
        self.buffer[self.write_pos] = Some(item);
        self.write_pos = (self.write_pos + 1) % self.capacity;
        if self.count < self.capacity {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Drain the buffer in order (oldest to newest).
    pub fn drain_ordered(&mut self) -> Vec<T> {
        let mut result = Vec::with_capacity(self.count);
        if self.count == 0 {
            return result;
        }

        // Calculate start position (oldest item)
        let start = if self.count == self.capacity {
            self.write_pos
        } else {
            0
        };

        for i in 0..self.count {
            let idx = (start + i) % self.capacity;
            if let Some(item) = self.buffer[idx].take() {
                result.push(item);
            }
        }

        self.write_pos = 0;
        self.count = 0;
        result
    }
}

/// Row buffer that maintains head rows and tail rows for large results.
#[derive(Debug)]
pub struct RowBuffer {
    /// First N rows (always kept).
    head: Vec<Vec<CellValue>>,
    /// Last M rows (ring buffer, overwrites oldest).
    tail: RingBuffer<Vec<CellValue>>,
    /// Maximum head size.
    head_capacity: usize,
    /// Total rows seen.
    total_count: usize,
}

impl RowBuffer {
    pub fn new(head_capacity: usize, tail_capacity: usize) -> Self {
        Self {
            head: Vec::with_capacity(head_capacity),
            tail: RingBuffer::new(tail_capacity),
            head_capacity,
            total_count: 0,
        }
    }

    /// Push a row into the buffer.
    pub fn push(&mut self, row: Vec<CellValue>) {
        self.total_count += 1;

        if self.head.len() < self.head_capacity {
            self.head.push(row);
        } else {
            self.tail.push(row);
        }
    }

    /// Total number of rows seen.
    pub fn total_count(&self) -> usize {
        self.total_count
    }

    /// Number of rows actually retained.
    pub fn retained_count(&self) -> usize {
        self.head.len() + self.tail.len()
    }

    /// Check if there are rows that were not retained (i.e., we need ellipsis).
    pub fn has_truncation(&self) -> bool {
        self.total_count > self.retained_count()
    }

    /// Number of rows skipped (shown as ellipsis).
    pub fn skipped_count(&self) -> usize {
        self.total_count.saturating_sub(self.retained_count())
    }

    /// Consume the buffer and return (head_rows, tail_rows).
    pub fn into_parts(mut self) -> (Vec<Vec<CellValue>>, Vec<Vec<CellValue>>) {
        let tail = self.tail.drain_ordered();
        (self.head, tail)
    }

    /// Check if buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.total_count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Alignment, ValueType};

    fn make_row(n: i32) -> Vec<CellValue> {
        vec![CellValue::new(n.to_string(), ValueType::Integer, Alignment::Right)]
    }

    #[test]
    fn test_ring_buffer_basic() {
        let mut rb = RingBuffer::new(3);
        assert!(rb.is_empty());

        rb.push(1);
        rb.push(2);
        assert_eq!(rb.len(), 2);

        rb.push(3);
        assert_eq!(rb.len(), 3);

        // Now it should start overwriting
        rb.push(4);
        assert_eq!(rb.len(), 3);

        let items = rb.drain_ordered();
        assert_eq!(items, vec![2, 3, 4]);
    }

    #[test]
    fn test_ring_buffer_exact_capacity() {
        let mut rb = RingBuffer::new(3);
        rb.push(1);
        rb.push(2);
        rb.push(3);

        let items = rb.drain_ordered();
        assert_eq!(items, vec![1, 2, 3]);
    }

    #[test]
    fn test_ring_buffer_under_capacity() {
        let mut rb = RingBuffer::new(5);
        rb.push(1);
        rb.push(2);

        let items = rb.drain_ordered();
        assert_eq!(items, vec![1, 2]);
    }

    #[test]
    fn test_row_buffer_small() {
        let mut buf = RowBuffer::new(3, 3);
        buf.push(make_row(1));
        buf.push(make_row(2));

        assert_eq!(buf.total_count(), 2);
        assert_eq!(buf.retained_count(), 2);
        assert!(!buf.has_truncation());

        let (head, tail) = buf.into_parts();
        assert_eq!(head.len(), 2);
        assert!(tail.is_empty());
    }

    #[test]
    fn test_row_buffer_fills_head() {
        let mut buf = RowBuffer::new(3, 3);
        for i in 1..=3 {
            buf.push(make_row(i));
        }

        assert_eq!(buf.total_count(), 3);
        assert!(!buf.has_truncation());

        let (head, tail) = buf.into_parts();
        assert_eq!(head.len(), 3);
        assert!(tail.is_empty());
    }

    #[test]
    fn test_row_buffer_uses_tail() {
        let mut buf = RowBuffer::new(2, 2);
        for i in 1..=4 {
            buf.push(make_row(i));
        }

        assert_eq!(buf.total_count(), 4);
        assert_eq!(buf.retained_count(), 4);
        assert!(!buf.has_truncation());

        let (head, tail) = buf.into_parts();
        assert_eq!(head.len(), 2); // rows 1, 2
        assert_eq!(tail.len(), 2); // rows 3, 4
    }

    #[test]
    fn test_row_buffer_truncation() {
        let mut buf = RowBuffer::new(2, 2);
        for i in 1..=10 {
            buf.push(make_row(i));
        }

        assert_eq!(buf.total_count(), 10);
        assert_eq!(buf.retained_count(), 4); // 2 head + 2 tail
        assert!(buf.has_truncation());
        assert_eq!(buf.skipped_count(), 6);

        let (head, tail) = buf.into_parts();
        assert_eq!(head.len(), 2);
        assert_eq!(tail.len(), 2);
        // Head should be 1, 2
        assert_eq!(head[0][0].display, "1");
        assert_eq!(head[1][0].display, "2");
        // Tail should be 9, 10 (last 2)
        assert_eq!(tail[0][0].display, "9");
        assert_eq!(tail[1][0].display, "10");
    }
}
