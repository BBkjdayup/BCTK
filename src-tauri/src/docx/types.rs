use std::ops::Range;

/// Half-open byte range into the original XML part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ByteSpan {
    pub start: usize,
    pub end: usize,
}

impl ByteSpan {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }

    pub const fn contains(self, other: Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }

    pub fn as_range(self) -> Range<usize> {
        self.start..self.end
    }
}
