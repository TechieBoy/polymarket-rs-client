//! End-to-End Test: CLOB Client fetching trades with Browser Wallet Signing
//!
//! This test interacts directly with the live Polymarket CLOB API (`https://clob.polymarket.com`),
//! using real credentials (private key and funder address) passed via environment variables:
//! - `PK`: The private key of the testing wallet
//! - `FUNDER`: The funder address to set for the `PolyGnosisSafe` signature type
//!
//! ⚠️ WARNING: This is an **end-to-end test** requiring real network access and valid credentials.
//! It will not run by default. To execute, run with `cargo test -- --ignored`.
//!
//! Example:
//! ```bash
//! PK=<your_private_key> FUNDER=<funder_address> cargo test test_client_get_trades_browser_wallet -- --ignored
//! ```
use alloy_primitives::Address;
use polymarket_rs_client::{
    ClobClient, SigType
};
use std::env;

const HOST: &str = "https://clob.polymarket.com";
const POLYGON: u64 = 137;

/// End-to-End test to verify fetching trades with a browser wallet signature setup
///
/// ## Purpose
/// - Initializes a `ClobClient` with L1 headers
/// - Generates or derives an API key
/// - Configures the client for `PolyGnosisSafe` signing
/// - Asserts successful connection and fetches trade data
///
/// ## Requirements
/// - Environment variable `PK` set with the private key
/// - Environment variable `FUNDER` set with the funder address
///
/// ## Run
/// - Ignored by default. Use `cargo test -- --ignored` to run this E2E test.
#[tokio::test]
#[ignore]
async fn test_client_get_trades_browser_wallet() {
    // Load sensitive environment variables
    let private_key = env::var("PK").expect("Missing PK env variable");
    let funder: Address = env::var("FUNDER")
        .expect("Missing FUNDER env variable")
        .parse()
        .expect("Failed to parse FUNDER as Address");

    // Initialize the CLOB client with L1 headers
    let nonce = None;
    let mut client = ClobClient::with_l1_headers(HOST, &private_key, POLYGON);

    // Derive or create API keys
    let keys = client.create_or_derive_api_key(nonce)
        .await
        .expect("Failed to create or derive API key");
    dbg!(&keys);

    // Set API credentials and configure order builder
    client.set_api_creds(keys);
    client.set_order_builder_params(Some(SigType::PolyGnosisSafe), Some(funder));

    // Assert client is ready
    assert!(client.get_ok().await, "Client failed health check with get_ok");

    // Fetch and print trades
    let trades = client.get_trades(None, None)
        .await
        .expect("Failed to fetch trades");
    println!("Trades: {:?}", trades);
}
