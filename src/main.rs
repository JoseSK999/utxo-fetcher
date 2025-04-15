mod coin_time;
mod error;

use crate::coin_time::fetch_coin_time;
use crate::error::FetchError;
use bitcoin::consensus::deserialize;
use bitcoin::consensus::encode::deserialize_hex;
use bitcoin::{Block, Transaction, TxOut};
use clap::Parser;
use serde::Deserialize;
use serde::Serialize;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::string::ToString;
use std::time::Duration;
use std::vec::Vec;
use std::{format, io};
use tokio::time::Instant;

pub const YELLOW: &str = "\x1b[33m";
pub const GREEN: &str = "\x1b[32m";
pub const RED: &str = "\x1b[31m";
pub const END: &str = "\x1b[0m";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
/// Represents an unspent transaction output (UTXO) with additional metadata for validation.
pub struct UtxoData {
    /// The unspent transaction output.
    pub txout: TxOut,
    /// Whether this output was created by a coinbase transaction.
    pub is_coinbase: bool,
    /// The block height at which the UTXO was confirmed.
    pub creation_height: u32,
    /// The creation time of the UTXO, defined by BIP 68 as the median time past (MTP) of the
    /// block preceding the confirming block.
    pub creation_time: u32,
}

#[derive(Debug, Parser)]
#[command(
    name = "utxo_fetcher",
    about = "Fetches spent UTXOs from a raw block and compresses both files."
)]
struct Cli {
    /// Directory containing the raw block file ("raw") and where the outputs will be saved.
    #[arg(value_name = "BLOCK_DIR")]
    block_dir: String,

    /// Optional block hash to verify that the raw block matches the expected hash.
    #[arg(value_name = "BLOCK_HASH")]
    block_hash: Option<String>,

    /// Compare spent_utxos.json against another file.
    /// Accepts a path to a .json file or a .zst file which will be decompressed first.
    #[arg(long, value_name = "UTXO_FILE")]
    eq: Option<PathBuf>,
}

/// Simple function to load UTXO data from json.
/// If the file has a .zst extension it will be decompressed.
fn load_utxo_data(path: impl AsRef<Path>) -> io::Result<Vec<UtxoData>> {
    let path = path.as_ref();
    let bytes = if path.extension().and_then(OsStr::to_str) == Some("zst") {
        // Decompress the .zst file and read its bytes.
        zstd::stream::decode_all(File::open(path)?)?
    } else {
        std::fs::read(path)?
    };
    Ok(serde_json::from_slice(&bytes)?)
}

/// Compares the UTXO data in the two files.
fn compare_utxos(current_file: &Path, eq_file: &Path) {
    let current_utxos = load_utxo_data(current_file).unwrap_or_else(|e| {
        eprintln!(
            "Error loading current UTXOs from {}: {}",
            current_file.display(),
            e
        );
        process::exit(1);
    });
    let eq_utxos = load_utxo_data(eq_file).unwrap_or_else(|e| {
        eprintln!(
            "Error loading comparison UTXOs from {}: {}",
            eq_file.display(),
            e
        );
        process::exit(1);
    });
    if current_utxos == eq_utxos {
        println!("{GREEN}UTXO files are equal{END}");
    } else {
        println!("{RED}UTXO files differ{END}");
    }
}

fn assert_block_hash(block: &Block, expected_hash: &str) {
    let actual_hash = block.block_hash().to_string();

    if actual_hash != expected_hash {
        eprintln!(
            "{RED}Block hashes do not match{END}\nExpected: {}\nActual:   {}",
            expected_hash, actual_hash
        );
        process::exit(1);
    }
}

fn deserialize_block(raw_file: &Path) -> Block {
    let raw_bytes = std::fs::read(raw_file).unwrap_or_else(|e| {
        eprintln!(
            "{RED}Error{END}: Couldn't read the 'raw' block file in '{}'. Err: {}",
            raw_file.display(),
            e,
        );
        process::exit(1);
    });

    deserialize(&raw_bytes).unwrap_or_else(|e| {
        eprintln!("{RED}Error{END}: Failed to deserialize block. Err: {}", e);
        process::exit(1);
    })
}

#[tokio::main]
async fn main() {
    // Parse the command-line arguments.
    let cli = Cli::parse();
    let dir = Path::new(&cli.block_dir);

    // Define the file paths.
    let raw_file = dir.join("raw");
    let spent_utxos_file = dir.join("spent_utxos.json");
    let raw_zst = dir.join("raw.zst");
    let spent_utxos_zst = dir.join("spent_utxos.zst");

    let block = deserialize_block(&raw_file);
    if let Some(expected_hash) = cli.block_hash {
        assert_block_hash(&block, &expected_hash);
    }

    // If we have the data already, and we want to compare it against another file, do it and return
    if spent_utxos_file.exists() && cli.eq.is_some() {
        compare_utxos(&spent_utxos_file, cli.eq.as_ref().unwrap());
        process::exit(0);
    }

    // Check if any output files already exist to avoid overwriting.
    if spent_utxos_file.exists() || raw_zst.exists() || spent_utxos_zst.exists() {
        eprintln!(
            "{YELLOW}Warning{END}: One or more output files already exist in '{}'. Aborting to avoid overwriting.",
            cli.block_dir
        );
        process::exit(1);
    }

    // Fetch, process and write the spent UTXOs.
    if let Err(e) = fetch_and_write_utxos(block, &spent_utxos_file).await {
        eprintln!("{RED}Error fetching spent UTXOs{END}: {}", e);
        process::exit(1);
    };
    if cli.eq.is_some() {
        compare_utxos(&spent_utxos_file, cli.eq.as_ref().unwrap());
    }

    // Compress the raw block file.
    if let Err(e) = compress_file(&raw_file, &raw_zst) {
        eprintln!("{RED}Error compressing the raw block file{END}: {}", e);
        process::exit(1);
    }
    // Compress the spent UTXOs file.
    if let Err(e) = compress_file(&spent_utxos_file, &spent_utxos_zst) {
        eprintln!("{RED}Error compressing the spent UTXOs file{END}: {}", e);
        process::exit(1);
    }

    println!("Block processed and both files have been compressed successfully.");
}

async fn request_from_url(client: &reqwest::Client, url: &str) -> Result<String, reqwest::Error> {
    let response = client.get(url).send().await?;
    response.text().await
}

async fn fetch_and_write_utxos(block: Block, file_path: &PathBuf) -> Result<(), FetchError> {
    let transactions = block.txdata;
    // We will query the chain API with this client
    let client = reqwest::Client::new();

    let mut utxos: Vec<UtxoData> = Vec::new();
    let mut coin_time_cache = HashMap::new();

    // Compute the total number of inputs (excluding coinbase) for progress reporting
    let total_inputs: usize = transactions[1..].iter().map(|tx| tx.input.len()).sum();
    let mut processed_inputs = 0;

    // Iterate through each transaction, except the coinbase
    for tx in &transactions[1..] {
        for txin in &tx.input {
            // Extract the UTXO location
            let txid = txin.previous_output.txid.to_string();
            let vout = txin.previous_output.vout;

            let start = Instant::now();
            let (utxo, cache_found) =
                fetch_utxo(&client, &txid, vout, &mut coin_time_cache).await?;
            let elapsed = start.elapsed();

            // We will sleep a bit if we were too fast, to respect API rate limits
            let desired_time = if cache_found {
                Duration::from_millis(120)
            } else {
                Duration::from_millis(320)
            };
            if elapsed < desired_time {
                tokio::time::sleep(desired_time - elapsed).await;
            }

            println!("\n{:#?}", utxo);
            utxos.push(utxo);
            processed_inputs += 1;

            let progress_percent = (processed_inputs as f64 / total_inputs as f64) * 100.0;
            println!(
                "{YELLOW}PROGRESS: {:.2}% ({}/{}){END}\n",
                progress_percent, processed_inputs, total_inputs
            );
        }
    }

    let file = File::create(file_path)?;
    // Serialize the UtxoData vector to JSON and write to a file
    serde_json::to_writer_pretty(file, &utxos)?;

    Ok(())
}

// Returns the fetched [UtxoData] and whether the unix time of the UTXO was found in the cache
async fn fetch_utxo(
    client: &reqwest::Client,
    txid: &str,
    vout: u32,
    coin_time_cache: &mut HashMap<u32, u32>,
) -> Result<(UtxoData, bool), FetchError> {
    println!("Fetching UTXO at {}:{}", txid, vout);

    let height = fetch_tx_height(client, txid).await?;
    if height < 11 {
        // UTXO height must be at least 11 to have 11 previous blocks (heights 0 to 10)
        return Err(FetchError::NotEnoughHeight(format!("{}:{}", txid, vout)));
    }
    let transaction = fetch_transaction(client, txid).await?;

    // Get the specific TxOut using the index
    let tx_out = transaction
        .output
        .get(vout as usize)
        .expect("Invalid vout index");

    let (coin_time, cache_found) = match coin_time_cache.entry(height) {
        Entry::Occupied(entry) => (*entry.get(), true),
        // If not cached, perform the computation and add to cache
        Entry::Vacant(entry) => {
            let computed = fetch_coin_time(client, height).await?;
            (*entry.insert(computed), false)
        }
    };

    let utxo = UtxoData {
        txout: tx_out.clone(),
        is_coinbase: transaction.is_coinbase(),
        creation_height: height,
        creation_time: coin_time,
    };

    Ok((utxo, cache_found))
}

async fn fetch_tx_height(client: &reqwest::Client, txid: &str) -> Result<u32, FetchError> {
    let url = format!("https://blockchain.info/rawtx/{}", txid);
    let response = request_from_url(client, &url)
        .await
        .map_err(FetchError::Height)?;

    let parsed: serde_json::Value = serde_json::from_str(&response)?;

    // Manually extract the height field
    let block_height = parsed["block_height"]
        .as_u64()
        .expect("Missing block_height value") as u32;

    Ok(block_height)
}

async fn fetch_transaction(
    client: &reqwest::Client,
    txid: &str,
) -> Result<Transaction, FetchError> {
    let url = format!("https://blockchain.info/rawtx/{}?format=hex", txid);
    let response = request_from_url(client, &url)
        .await
        .map_err(FetchError::Transaction)?;

    let transaction: Transaction = deserialize_hex(&response)?;

    Ok(transaction)
}

fn compress_file(input_path: &PathBuf, output_path: &PathBuf) -> io::Result<()> {
    let raw_bytes = std::fs::read(input_path)?;

    // Compress the data with a compression level (1 is fast, 22 is maximum compression)
    let compressed_data = zstd::encode_all(&raw_bytes[..], 22)?;

    // Write the compressed data to a file
    let mut compressed_file = File::create(output_path)?;
    compressed_file.write_all(&compressed_data)?;

    println!("Compression complete for {}!", output_path.display());
    Ok(())
}
