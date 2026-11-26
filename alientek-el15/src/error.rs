use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Bluetooth error: {0}")]
    Bluetooth(#[from] btleplug::Error),

    #[error("No Bluetooth adapter found")]
    NoAdapter,

    #[error("Device not connected")]
    NotConnected,

    #[error("Device disconnected")]
    Disconnected,

    #[error("Required GATT characteristic not found ({0})")]
    CharacteristicNotFound(&'static str),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Timeout waiting for {0}")]
    Timeout(&'static str),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
