use thiserror;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("URI error: {0}")]
    BadUri(String),
    #[error("URI validation error: {0}")]
    InvalidUri(#[from] iri_string::validate::Error),
    #[error("Archive error: {0}")]
    BadArchive(String),
    #[error("Decompression error: {0}")]
    Decompression(String),
    #[error("S3 error: {0}")]
    S3(String),
}

impl Error {
    /// The resource doesn't exist, as opposed to a read that failed and might
    /// succeed if retried
    pub fn is_not_found(&self) -> bool {
        matches!(self, Error::NotFound(_))
    }
}
