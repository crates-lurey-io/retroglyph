//! 2D pseudographic terminal
//!
//! # Example
//!
//! ```
//! use retroglyph::add;
//!
//! assert_eq!(add(2, 3), 5);
//! ```

// TODO: Write a library.
#[must_use]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(add(2, 3), 5);
    }
}
