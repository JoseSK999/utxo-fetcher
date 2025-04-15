use bitcoin::consensus::encode::FromHexError;
use std::{fmt, io};

/// High level error type for the UTXO fetching functionality
pub enum FetchError {
    /// Generic I/O error
    Io(io::Error),
    /// Error while deserializing the transaction data
    FromHex(FromHexError),
    /// Error while fetching the block height of the transaction
    Height(reqwest::Error),
    /// Error while fetching the transaction
    Transaction(reqwest::Error),
    /// Error while fetching data for the coin time computation
    CoinTime(reqwest::Error),
    /// UTXO has less than 11 previous blocks in the chain
    NotEnoughHeight(String),
}

impl From<io::Error> for FetchError {
    fn from(e: io::Error) -> Self {
        FetchError::Io(e)
    }
}

impl From<serde_json::Error> for FetchError {
    fn from(e: serde_json::Error) -> Self {
        FetchError::Io(e.into())
    }
}

impl From<FromHexError> for FetchError {
    fn from(e: FromHexError) -> Self {
        FetchError::FromHex(e)
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::Io(err) => write!(f, "I/O error: {}", err),
            FetchError::FromHex(err) => write!(f, "Transaction deserialization error: {}", err),
            FetchError::Height(err) => write!(f, "Height fetching error: {}", err),
            FetchError::Transaction(err) => write!(f, "Transaction fetching error: {}", err),
            FetchError::CoinTime(err) => write!(f, "CoinTime fetching error: {}", err),
            FetchError::NotEnoughHeight(utxo) => {
                write!(f, "UTXO has a height less than 11: {}", utxo)
            }
        }
    }
}
