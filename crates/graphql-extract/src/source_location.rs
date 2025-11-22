/// Position in a source file (0-indexed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    /// Line number (0-indexed)
    pub line: usize,
    /// Column number (0-indexed, UTF-16 code units)
    pub column: usize,
}

impl Position {
    #[must_use]
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// Range in a source file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    #[must_use]
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

/// Source location information for extracted GraphQL
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// Byte offset in the original source file
    pub offset: usize,
    /// Length in bytes
    pub length: usize,
    /// Range in the original source file
    pub range: Range,
}

impl SourceLocation {
    #[must_use]
    pub const fn new(offset: usize, length: usize, range: Range) -> Self {
        Self {
            offset,
            length,
            range,
        }
    }
}
