use crate::Position;

/// Fast line-to-offset and offset-to-line conversion using a pre-built index.
///
/// This index eliminates the O(N) character iteration required for position-to-offset
/// conversions by maintaining a cached list of line start offsets.
///
/// # Performance
///
/// - Build time: O(N) where N is the length of the source text
/// - Memory: O(L) where L is the number of lines (~8 bytes per line)
/// - Lookup time: O(1) for `position_to_offset`
/// - Lookup time: O(log L) for `offset_to_position` (binary search)
///
/// # Example
///
/// ```
/// use graphql_project::{Position, LineIndex};
///
/// let source = "line 0\nline 1\nline 2";
/// let index = LineIndex::new(source);
///
/// // Convert position to byte offset
/// let offset = index.position_to_offset(Position { line: 1, character: 0 }).unwrap();
/// assert_eq!(offset, 7); // Start of "line 1"
///
/// // Convert byte offset to position
/// let pos = index.offset_to_position(7);
/// assert_eq!(pos.line, 1);
/// assert_eq!(pos.character, 0);
/// ```
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the start of each line
    /// Index 0 is always 0 (start of file)
    /// Index N is the byte offset immediately after the Nth '\n' character
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build a line index from source text
    ///
    /// This scans the entire source once to find all '\n' characters
    /// and records their byte offsets.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        let mut offset = 0;

        for ch in text.chars() {
            offset += ch.len_utf8();
            if ch == '\n' {
                line_starts.push(offset);
            }
        }

        Self { line_starts }
    }

    /// Convert a line/column position to a byte offset
    ///
    /// Returns `None` if the position is out of bounds.
    ///
    /// # Complexity
    ///
    /// O(1) - Direct array lookup and addition
    #[must_use]
    pub fn position_to_offset(&self, position: Position) -> Option<usize> {
        let line_start = *self.line_starts.get(position.line)?;
        Some(line_start + position.character)
    }

    /// Convert a byte offset to a line/column position
    ///
    /// # Complexity
    ///
    /// O(log L) where L is the number of lines - uses binary search
    #[must_use]
    pub fn offset_to_position(&self, offset: usize) -> Position {
        // Binary search to find the line containing this offset
        let line = match self.line_starts.binary_search(&offset) {
            // Exact match: offset is at the start of a line
            Ok(line) => line,
            // Not found: offset is somewhere within a line
            Err(line) => line.saturating_sub(1),
        };

        let line_start = self.line_starts[line];
        Position {
            line,
            character: offset.saturating_sub(line_start),
        }
    }

    /// Get the number of lines in the indexed text
    #[must_use]
    pub const fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Get the byte offset of the start of a line
    ///
    /// Returns `None` if the line number is out of bounds.
    #[must_use]
    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.line_starts.get(line).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_string() {
        let index = LineIndex::new("");
        assert_eq!(index.line_count(), 1);
        assert_eq!(index.line_start(0), Some(0));

        let pos = index.position_to_offset(Position {
            line: 0,
            character: 0,
        });
        assert_eq!(pos, Some(0));
    }

    #[test]
    fn test_single_line() {
        let source = "hello world";
        let index = LineIndex::new(source);

        assert_eq!(index.line_count(), 1);

        // Start of line
        let offset = index
            .position_to_offset(Position {
                line: 0,
                character: 0,
            })
            .unwrap();
        assert_eq!(offset, 0);

        // Middle of line
        let offset = index
            .position_to_offset(Position {
                line: 0,
                character: 6,
            })
            .unwrap();
        assert_eq!(offset, 6);

        // Reverse: offset to position
        let pos = index.offset_to_position(6);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 6);
    }

    #[test]
    fn test_multiple_lines() {
        let source = "line 0\nline 1\nline 2";
        let index = LineIndex::new(source);

        assert_eq!(index.line_count(), 3);

        // Line 0
        let offset = index
            .position_to_offset(Position {
                line: 0,
                character: 0,
            })
            .unwrap();
        assert_eq!(offset, 0);

        // Line 1 start (immediately after '\n' at offset 6)
        let offset = index
            .position_to_offset(Position {
                line: 1,
                character: 0,
            })
            .unwrap();
        assert_eq!(offset, 7);

        // Line 2 start
        let offset = index
            .position_to_offset(Position {
                line: 2,
                character: 0,
            })
            .unwrap();
        assert_eq!(offset, 14);

        // Middle of line 1
        let offset = index
            .position_to_offset(Position {
                line: 1,
                character: 5,
            })
            .unwrap();
        assert_eq!(offset, 12); // "line 1"[5] = '1'
    }

    #[test]
    fn test_offset_to_position() {
        let source = "line 0\nline 1\nline 2";
        let index = LineIndex::new(source);

        // Offset 0: start of line 0
        let pos = index.offset_to_position(0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);

        // Offset 7: start of line 1
        let pos = index.offset_to_position(7);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);

        // Offset 10: middle of line 1 ("line 1"[3] = 'e')
        let pos = index.offset_to_position(10);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 3);

        // Offset 14: start of line 2
        let pos = index.offset_to_position(14);
        assert_eq!(pos.line, 2);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn test_out_of_bounds() {
        let source = "line 0\nline 1";
        let index = LineIndex::new(source);

        // Line 10 doesn't exist
        let offset = index.position_to_offset(Position {
            line: 10,
            character: 0,
        });
        assert_eq!(offset, None);

        // Character 1000 on line 0 is out of bounds, but we don't validate that
        // (validation would require knowing the line length)
        let offset = index.position_to_offset(Position {
            line: 0,
            character: 1000,
        });
        assert!(offset.is_some()); // Returns Some, but would be invalid
    }

    #[test]
    fn test_utf8_characters() {
        // "Hello 世界" where 世 and 界 are 3 bytes each
        let source = "Hello 世界\nSecond line";
        let index = LineIndex::new(source);

        // The newline is at byte offset 12 (5 ASCII + 1 space + 6 UTF-8)
        let line1_start = index.line_start(1).unwrap();
        assert_eq!(line1_start, 13);

        // Position at start of line 1
        let offset = index
            .position_to_offset(Position {
                line: 1,
                character: 0,
            })
            .unwrap();
        assert_eq!(offset, 13);
    }

    #[test]
    fn test_windows_line_endings() {
        let source = "line 0\r\nline 1\r\nline 2";
        let index = LineIndex::new(source);

        // \r\n is two characters, so line 1 starts at offset 8
        let offset = index
            .position_to_offset(Position {
                line: 1,
                character: 0,
            })
            .unwrap();
        assert_eq!(offset, 8);

        // Line 2 starts at offset 16
        let offset = index
            .position_to_offset(Position {
                line: 2,
                character: 0,
            })
            .unwrap();
        assert_eq!(offset, 16);
    }

    #[test]
    fn test_roundtrip() {
        let source = "fn main() {\n    println!(\"Hello\");\n}\n";
        let index = LineIndex::new(source);

        // Test various positions roundtrip correctly
        let test_cases = vec![
            Position {
                line: 0,
                character: 0,
            },
            Position {
                line: 0,
                character: 5,
            },
            Position {
                line: 1,
                character: 4,
            },
            Position {
                line: 2,
                character: 0,
            },
        ];

        for pos in test_cases {
            let offset = index.position_to_offset(pos).unwrap();
            let roundtrip = index.offset_to_position(offset);
            assert_eq!(
                pos, roundtrip,
                "Position {pos:?} -> offset {offset} -> position {roundtrip:?}",
            );
        }
    }
}
