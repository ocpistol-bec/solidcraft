//! Typed errors for every layer of an Inventor document.

use std::fmt;

/// Result alias used throughout Corliss.
pub type Result<T> = std::result::Result<T, CorlissError>;

/// What went wrong, independent of where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureClass {
    /// The data ended before the structure did.
    Truncated {
        /// Bytes the field needed.
        needed: usize,
        /// Bytes left in the input.
        available: usize,
    },
    /// A field holds a value the specification does not allow.
    InvalidValue {
        /// The offending value, printed.
        found: String,
        /// The value or shape the specification requires.
        expected: String,
    },
    /// Bytes remain after a structure that must consume its whole input.
    TrailingBytes {
        /// Number of unread bytes.
        count: usize,
    },
    /// A stream or storage the operation needs is not in the container.
    Missing {
        /// Path or name of the missing item.
        what: String,
    },
    /// A structure that the specification lists but no sample has shown (layout unknown).
    Unsupported {
        /// Why the structure cannot be handled.
        reason: String,
    },
    /// The compressed data could not be inflated or deflated.
    Compression {
        /// Error text of the codec.
        detail: String,
    },
    /// The compound-file layer or the file system failed.
    Io {
        /// Error text of the I/O layer.
        detail: String,
    },
}

/// An error with the operation, the structure and the byte offset it concerns.
///
/// Example: `structure = "M stream outer header"`, `field = "magic"`, `offset = 0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorlissError {
    /// The operation that failed ("read", "write", "decode", "encode").
    pub operation: &'static str,
    /// The structure being processed, named as in the specification.
    pub structure: &'static str,
    /// The field of the structure, or "" when the whole structure is meant.
    pub field: &'static str,
    /// Byte offset in the structure's input, when it is known.
    pub offset: Option<usize>,
    /// The failure class with its details.
    pub class: FailureClass,
}

impl CorlissError {
    /// Builds a decode error for `structure.field` at `offset`.
    pub fn decode(
        structure: &'static str,
        field: &'static str,
        offset: usize,
        class: FailureClass,
    ) -> Self {
        CorlissError {
            operation: "decode",
            structure,
            field,
            offset: Some(offset),
            class,
        }
    }

    /// Builds an error for a field whose value is out of the specified range.
    pub fn invalid(
        structure: &'static str,
        field: &'static str,
        offset: usize,
        found: impl fmt::Display,
        expected: impl fmt::Display,
    ) -> Self {
        let class = FailureClass::InvalidValue {
            found: found.to_string(),
            expected: expected.to_string(),
        };
        Self::decode(structure, field, offset, class)
    }

    /// Builds an error for a stream or storage that is not in the container.
    pub fn missing(
        operation: &'static str,
        structure: &'static str,
        what: impl Into<String>,
    ) -> Self {
        CorlissError {
            operation,
            structure,
            field: "",
            offset: None,
            class: FailureClass::Missing { what: what.into() },
        }
    }

    /// Builds an error for an I/O failure of `operation` on `structure`.
    pub fn io(operation: &'static str, structure: &'static str, err: &std::io::Error) -> Self {
        CorlissError {
            operation,
            structure,
            field: "",
            offset: None,
            class: FailureClass::Io {
                detail: err.to_string(),
            },
        }
    }
}

impl fmt::Display for CorlissError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.operation, self.structure)?;
        if !self.field.is_empty() {
            write!(f, ".{}", self.field)?;
        }
        if let Some(offset) = self.offset {
            write!(f, " at offset {offset}")?;
        }
        write!(f, ": {}", self.class)
    }
}

impl fmt::Display for FailureClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { needed, available } => {
                write!(f, "needs {needed} bytes, {available} available")
            }
            Self::InvalidValue { found, expected } => {
                write!(f, "found {found}, expected {expected}")
            }
            Self::TrailingBytes { count } => write!(f, "{count} unread bytes after the structure"),
            Self::Missing { what } => write!(f, "{what} is missing"),
            Self::Unsupported { reason } => write!(f, "unsupported: {reason}"),
            Self::Compression { detail } => write!(f, "compression failed: {detail}"),
            Self::Io { detail } => write!(f, "I/O failed: {detail}"),
        }
    }
}

impl std::error::Error for CorlissError {}
