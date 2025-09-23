use std::error::Error as StdError;
use std::fmt;
use std::io;

use crate::sanity::SanityCheckError;

/// Top-level error type for filter-repo-rs operations.
///
/// This consolidates the different error sources used by the application
/// and preserves the human-friendly formatting already implemented by
/// [`SanityCheckError`].
#[derive(Debug)]
pub enum FilterRepoError {
    /// Wrapper around lower-level I/O failures.
    Io(io::Error),
    /// Rich sanity-check failure reporting.
    Sanity(SanityCheckError),
    /// Invalid option or configuration supplied by the caller.
    InvalidOptions(String),
}

impl fmt::Display for FilterRepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilterRepoError::Io(err) => write!(f, "{err}"),
            FilterRepoError::Sanity(err) => write!(f, "{err}"),
            FilterRepoError::InvalidOptions(msg) => f.write_str(msg),
        }
    }
}

impl StdError for FilterRepoError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            FilterRepoError::Io(err) => Some(err),
            FilterRepoError::Sanity(err) => err.source(),
            FilterRepoError::InvalidOptions(_) => None,
        }
    }
}

impl From<io::Error> for FilterRepoError {
    fn from(err: io::Error) -> Self {
        FilterRepoError::Io(err)
    }
}

impl From<SanityCheckError> for FilterRepoError {
    fn from(err: SanityCheckError) -> Self {
        FilterRepoError::Sanity(err)
    }
}

impl From<FilterRepoError> for io::Error {
    fn from(err: FilterRepoError) -> Self {
        match err {
            FilterRepoError::Io(inner) => inner,
            other => io::Error::new(io::ErrorKind::Other, other),
        }
    }
}

impl FilterRepoError {
    /// Convenience constructor for invalid option failures.
    pub fn invalid_options(msg: impl Into<String>) -> Self {
        FilterRepoError::InvalidOptions(msg.into())
    }
}

/// Convenience result alias using [`FilterRepoError`].
pub type Result<T> = std::result::Result<T, FilterRepoError>;
