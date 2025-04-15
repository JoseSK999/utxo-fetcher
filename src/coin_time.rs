use crate::error::FetchError;
use crate::request_from_url;
use crate::END;
use crate::GREEN;

/// Fetches the “coin time” for a UTXO by computing the median-time-past (MTP) of the 11 blocks
/// immediately preceding the current block (i.e. from height h–11 to h–1).
///
/// In Bitcoin’s consensus rules (BIP 68), the creation time (mining date) of an output is defined
/// as the MTP of the block immediately before the block that mined it. Here we fetch all 11
/// timestamps and then compute the median (middle element when the timestamps are sorted).
pub async fn fetch_coin_time(
    client: &reqwest::Client,
    current_height: u32,
) -> Result<u32, FetchError> {
    println!(
        "Fetching timestamps for heights: ({}..={})",
        color_last_3_digits(current_height - 11),
        color_last_3_digits(current_height - 1),
    );

    // Run tasks concurrently:
    // - Fetch 10 blocks from (current_height - 11) to (current_height - 2).
    // - Fetch the block at (current_height - 1).
    let (mut timestamps, last_block_timestamp) = futures::try_join!(
        fetch_batch_timestamps(client, current_height - 2),
        fetch_last_block_timestamp(client, current_height - 1)
    )?;

    // Get the vector with the previous 11 timestamps, sort it, and get the median value.
    assert_eq!(timestamps.len(), 10);
    timestamps.push(last_block_timestamp);
    timestamps.sort();

    print_timestamps(&timestamps);
    // For 11 timestamps, the median is at index 5.
    let median_time = timestamps[5];
    Ok(median_time)
}

/// Fetches the timestamps for the previous 10 blocks, including the `top_height` block.
/// It uses the endpoint: GET https://blockstream.info/api/blocks/{top_height}
async fn fetch_batch_timestamps(
    client: &reqwest::Client,
    top_height: u32,
) -> Result<Vec<u32>, FetchError> {
    let blocks_url = format!("https://blockstream.info/api/blocks/{}", top_height);
    let response = request_from_url(client, &blocks_url)
        .await
        .map_err(FetchError::CoinTime)?;
    let blocks: Vec<serde_json::Value> = serde_json::from_str(&response)?;

    // Extract timestamps from each block.
    let timestamps = blocks
        .into_iter()
        .enumerate()
        .map(|(i, block)| {
            let height = block["height"].as_u64().unwrap() as u32;
            assert_eq!(top_height - i as u32, height); // Ensure we are reading the previous blocks

            block["timestamp"]
                .as_u64()
                .expect("Timestamp missing in block") as u32
        })
        .collect();
    Ok(timestamps)
}

/// Fetches the timestamp of a single block given its height using two endpoints:
/// 1. GET https://blockstream.info/api/block-height/{height} returns the block hash.
/// 2. GET https://blockstream.info/api/block/{hash} returns the block details as JSON.
async fn fetch_last_block_timestamp(
    client: &reqwest::Client,
    height: u32,
) -> Result<u32, FetchError> {
    let block_height_url = format!("https://blockstream.info/api/block-height/{}", height);
    let hash_response = request_from_url(client, &block_height_url)
        .await
        .map_err(FetchError::CoinTime)?;
    let block_hash = hash_response.trim();

    let block_url = format!("https://blockstream.info/api/block/{}", block_hash);
    let block_response = request_from_url(client, &block_url)
        .await
        .map_err(FetchError::CoinTime)?;
    let block: serde_json::Value = serde_json::from_str(&block_response)?;

    let timestamp = block["timestamp"]
        .as_u64()
        .expect("Timestamp missing in block") as u32;
    Ok(timestamp)
}

fn print_timestamps(timestamps: &[u32]) {
    assert_eq!(timestamps.len(), 11);
    // The median of 11 elements is at index 5.
    let median_index = 5;

    print!("Median Time: [");
    for (i, ts) in timestamps.iter().enumerate() {
        // Check if we are at the middle element.
        if i == median_index {
            // Print the middle element in green.
            print!("{GREEN}{ts}{END}");
        } else {
            print!("{ts}");
        }

        // Print a comma separator for all elements except the last.
        if i < 11 - 1 {
            print!(", ");
        }
    }
    println!("]");
}

fn color_last_3_digits(num: u32) -> String {
    let num_str = num.to_string();
    let len = num_str.len();
    if len > 3 {
        let (prefix, suffix) = num_str.split_at(len - 3);
        // Concatenate the prefix (uncolored) with the colored suffix, then reset the color.
        format!("{prefix}{GREEN}{suffix}{END}")
    } else {
        // If the number has three or fewer digits, color the entire string.
        format!("{GREEN}{num_str}{END}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    /// Validate that a timestamp refers to the expected UTC date and return the unix value.
    fn assert_date(unix_timestamp: u32, date: &str) -> u32 {
        let dt = DateTime::from_timestamp(unix_timestamp as i64, 0).unwrap();

        assert_eq!(
            dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            date,
            "The timestamp doesn't match the expected date",
        );
        unix_timestamp
    }

    async fn assert_coin_time(client: &reqwest::Client, height: u32, expected_coin_time: u32) {
        // Call fetch_coin_time, which makes real HTTP requests.
        match fetch_coin_time(client, height).await {
            Ok(coin_time) => {
                assert_eq!(
                    coin_time, expected_coin_time,
                    "Coin time for height {} does not match: expected {}, got {}",
                    height, expected_coin_time, coin_time
                );
            }
            Err(e) => {
                panic!("fetch_coin_time failed with error: {}", e);
            }
        }
    }

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
}
