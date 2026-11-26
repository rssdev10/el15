use thiserror::Error;

/// Errors from scanning, connecting and sending.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The Bluetooth stack reported an error.
    #[error("Bluetooth error: {0}")]
    Bluetooth(#[from] btleplug::Error),

    /// The system has no Bluetooth adapter.
    #[error("No Bluetooth adapter found")]
    NoAdapter,

    /// The connected peripheral lacks the EL15 characteristic named here.
    #[error("Required GATT characteristic not found ({0})")]
    CharacteristicNotFound(&'static str),
}

/// `Result` with this crate's [`Error`](enum@Error).
pub type Result<T> = std::result::Result<T, Error>;
