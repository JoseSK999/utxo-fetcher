# UTXO Fetcher

A CLI tool for fetching spent UTXOs from a raw Bitcoin block and compressing both the block and UTXO data. This tool is particularly useful for generating test data for the [Floresta](https://github.com/vinteumorg/Floresta) software, which validates transactions and blocks using a spent UTXO data map (`HashMap<OutPoint, UtxoData>`).

> Currently, this tool is only fetching the data by calling the `blockstream.info` and `blockchain.info` APIs, which is not ideal for performance and security.

## Overview

`utxo_fetcher` processes a raw block file to extract UTXO information, including:

- **Spent Transaction Output** (`TxOut`)
- **Coinbase Status:** Indicates if the UTXO came from a coinbase transaction
- **Creation Height:** The block height at which the UTXO was confirmed
- **Creation Time (Coin Time):** The coin mining date computed as defined by [BIP 68](https://github.com/bitcoin/bips/blob/master/bip-0068.mediawiki)

The **coin time** is the less trivial part to obtain. It is calculated as the median time past (MTP) of the block preceding the confirming block. This computation is handled by the `coin_time` module via the `fetch_coin_time` function. The result is cached for performance across multiple UTXO lookups.

## Features

- **UTXO Extraction:** Iterates over transaction inputs (excluding coinbase) to fetch the referenced UTXO data.
- **Coin Time Calculation:** Computes the coin time per BIP 68 (using MTP of the previous block).
- **Block Hash Verification:** Optionally verifies the raw block hash against an expected hash.
- **UTXO Comparison:** Optionally compares generated UTXO data with an external JSON or Zstandard-compressed file.
- **File Compression:** Compresses both the raw block file and the generated UTXO JSON file using Zstandard (Zstd).

## Installation

Ensure you have [Rust](https://rust-lang.org/) installed. Clone the repository and build the project with Cargo:

```bash
git clone https://github.com/JoseSK999/utxo-fetcher.git
cd utxo_fetcher
cargo build --release
```

### Usage

Run the CLI tool as follows:

```bash
cargo run --release <BLOCK_DIR> [BLOCK_HASH] [--eq <UTXO_FILE>]
```

- `<BLOCK_DIR>`: Directory containing the raw block file named `raw`. The tool outputs `spent_utxos.json`, `raw.zst`, and `spent_utxos.zst` in this directory.

- `[BLOCK_HASH]`: (_Optional_) Expected block hash to verify the raw block's integrity.

- `--eq <UTXO_FILE>`: (_Optional_) Path to a JSON or `.zst` file to compare against the generated UTXO data.

#### Example:

Assuming you have the `raw` block file at `./blocks/block123`, the expected block hash is "abcdef1234567890", and we want to compare the resulting UTXO data vector against a `data/comparison_utxos.json`:

```bash
cargo run --release ./blocks/block123 abcdef1234567890 --eq data/comparison_utxos.json
```

### Coin Time Tests

There is a unit test for the `coin_time` module, which you can run with `cargo test --release`.

The test vectors are the following:

```rust
#[tokio::test]
async fn test_fetch_coin_time() {
    let client = reqwest::Client::new();

    let height = 866_339;
    // You can verify that blocks 866,328 to 866,338 have ascending timestamps, and the block
    // at the middle (i.e. block 866,333) has this exact timestamp. This is the median of the
    // previous 11 blocks, which is the coin time for block 866,339.
    let expected_coin_time = assert_date(1_729_331_091, "2024-10-19 09:44:51");
    assert_coin_time(&client, height, expected_coin_time).await;

    let height = 156_119;
    // From blocks 156,108 to 156,118 the middle block would be 156,113. However, this block
    // has a timestamp of roughly two hours more than the rest of blocks. This makes it the
    // highest timestamp in the sorted vector, so the median is that of block 156,114 instead.
    //
    // Timestamp order: 113 > 118 > 117 > 116 > 115 > [114] > 112 > 111 > 110 > 109 > 108
    let expected_coin_time = assert_date(1_323_065_878, "2011-12-05 06:17:58");
    assert_coin_time(&client, height, expected_coin_time).await;

    // Try with a height that is one less, effectively moving the median block to 156,112
    let expected_coin_time = assert_date(1_323_065_825, "2011-12-05 06:17:05");
    assert_coin_time(&client, height - 1, expected_coin_time).await;

    // By adding one, we shift the median block to 156,115
    let expected_coin_time = assert_date(1_323_066_065, "2011-12-05 06:21:05");
    assert_coin_time(&client, height + 1, expected_coin_time).await;
}
```
