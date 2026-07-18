use serde::{Deserialize, Serialize};

/// Categorisation of which input-level limit was exceeded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LimitKind {
    InputBytes,
    PixelCount,
    Dimension,
}

impl std::fmt::Display for LimitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LimitKind::InputBytes => write!(f, "input bytes"),
            LimitKind::PixelCount => write!(f, "pixel count"),
            LimitKind::Dimension => write!(f, "dimension"),
        }
    }
}

/// The single error type for the Spryteo core and all pipeline crates.
///
/// Every crate in the workspace returns this error (wrapped via `thiserror`'s
/// `#[from]` or direct construction).  All variants are `Send + Sync` so they
/// can be returned across FFI boundaries without panicking to construct.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum SpryteoError {
    /// The input file could not be decoded or its format is not supported.
    #[error("Invalid or unsupported input: {0}")]
    InvalidInput(String),

    /// One of the decompression-bomb limits (bytes, pixels, or dimension) was
    /// exceeded.  The `kind` field identifies which limit, `value` is the
    /// actual (offending) quantity, and `max` is the configured cap.
    #[error("Limit exceeded: {kind} value {value} exceeds maximum {max}")]
    LimitExceeded {
        kind: LimitKind,
        value: u64,
        max: u64,
    },

    /// The conversion did not finish within the configured timeout.
    #[error("Operation timed out")]
    Timeout,

    /// The conversion was cancelled (e.g. by a user or API signal).
    #[error("Operation cancelled")]
    Cancelled,

    /// An unexpected internal error that does not fit any other variant.
    /// The `String` payload is a human-readable description; never a panic.
    #[error("Internal error: {0}")]
    Internal(String),
}
