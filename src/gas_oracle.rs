//! Real gas-price oracle for Ethereum mainnet.
//!
//! `seamint` never invents gas multipliers. For Ethereum mainnet it reads the
//! same real slow/medium/fast values Etherscan shows on its gas tracker
//! (SafeGasPrice / ProposeGasPrice / FastGasPrice) via the public V2 API. The
//! public endpoint works without a key at a 1 req / 5s rate limit; an optional
//! `ETHERSCAN_API_KEY` lifts that limit. Cheap chains (Robinhood, Ink) have no
//! slow/medium/fast breakdown on their explorers, so the RPC's own fee
//! estimate is used there instead.

use alloy_primitives::U256;
use reqwest::Client;
use std::sync::OnceLock;
use thiserror::Error;

const ETHERSCAN_GAS_ORACLE_URL: &str =
    "https://api.etherscan.io/v2/api?chainid=1&module=gastracker&action=gasoracle";
const GWEI_IN_WEI: u64 = 1_000_000_000;

#[derive(Error, Debug)]
pub enum GasOracleError {
    #[error("Etherscan gas tracker request failed: {0}")]
    Http(String),
    #[error("Etherscan gas tracker returned an unexpected shape: {0}")]
    Parse(String),
}

/// Real Ethereum mainnet gas prices in gwei (total gas price per tier), the
/// same values Etherscan displays.
#[derive(Clone, Copy, Debug)]
pub struct EthGasOracle {
    /// SafeGasPrice (low tier).
    pub safe_gas_price_gwei: f64,
    /// ProposeGasPrice (standard tier).
    pub propose_gas_price_gwei: f64,
    /// FastGasPrice (fast tier).
    pub fast_gas_price_gwei: f64,
}

impl EthGasOracle {
    pub fn tier(&self, level: crate::domain::GasFeeLevel) -> f64 {
        match level {
            crate::domain::GasFeeLevel::Slow => self.safe_gas_price_gwei,
            crate::domain::GasFeeLevel::Medium => self.propose_gas_price_gwei,
            crate::domain::GasFeeLevel::Fast => self.fast_gas_price_gwei,
        }
    }
}

#[derive(serde::Deserialize)]
struct GasOracleResponse {
    status: String,
    result: GasOracleResult,
}

#[derive(serde::Deserialize)]
struct GasOracleResult {
    #[serde(rename = "SafeGasPrice")]
    safe_gas_price: String,
    #[serde(rename = "ProposeGasPrice")]
    propose_gas_price: String,
    #[serde(rename = "FastGasPrice")]
    fast_gas_price: String,
}

fn parse_gwei_str(value: &str) -> Result<f64, GasOracleError> {
    value
        .parse::<f64>()
        .map_err(|_| GasOracleError::Parse(value.to_string()))
}

/// Fetch the real Ethereum mainnet slow/medium/fast gas prices from Etherscan.
pub async fn fetch_eth_gas_oracle() -> Result<EthGasOracle, GasOracleError> {
    let api_key = std::env::var("ETHERSCAN_API_KEY").ok();
    let url = match api_key {
        Some(key) => format!("{ETHERSCAN_GAS_ORACLE_URL}&apikey={key}"),
        None => ETHERSCAN_GAS_ORACLE_URL.to_string(),
    };

    let client = Client::new();
    let response = client
        .get(&url)
        .header("accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|err| GasOracleError::Http(err.to_string()))?;

    if !response.status().is_success() {
        return Err(GasOracleError::Http(format!(
            "HTTP {}",
            response.status()
        )));
    }

    let body = response
        .bytes()
        .await
        .map_err(|err| GasOracleError::Http(err.to_string()))?;
    let parsed: GasOracleResponse = serde_json::from_slice(&body)
        .map_err(|err| GasOracleError::Parse(err.to_string()))?;

    if parsed.status != "1" {
        return Err(GasOracleError::Parse(format!(
            "status {} (rate limited without an API key; set ETHERSCAN_API_KEY)",
            parsed.status
        )));
    }

    Ok(EthGasOracle {
        safe_gas_price_gwei: parse_gwei_str(&parsed.result.safe_gas_price)?,
        propose_gas_price_gwei: parse_gwei_str(&parsed.result.propose_gas_price)?,
        fast_gas_price_gwei: parse_gwei_str(&parsed.result.fast_gas_price)?,
    })
}

/// Convert gwei (as a float) to wei, rounding up so the transaction is never
/// under-funded.
pub fn gwei_f64_to_wei(gwei: f64) -> U256 {
    let wei_f64 = gwei * GWEI_IN_WEI as f64;
    let rounded = wei_f64.ceil() as u64;
    U256::from(rounded)
}

/// Process-wide cache of the Etherscan oracle. The public endpoint rate-limits
/// to 1 request / 5s, and a single mint computes fees many times, so we fetch
/// once and reuse the cached result for the whole process.
static ETH_ORACLE_CACHE: OnceLock<Option<EthGasOracle>> = OnceLock::new();

/// Return the cached Ethereum mainnet gas oracle, fetching it lazily on the
/// first call. Never blocks a mint on the oracle: on any failure it returns
/// `None` and the caller falls back to the RPC's own real estimate.
pub async fn eth_gas_oracle() -> Option<EthGasOracle> {
    if let Some(cached) = ETH_ORACLE_CACHE.get() {
        return *cached;
    }
    let fetched = fetch_eth_gas_oracle().await.ok();
    let _ = ETH_ORACLE_CACHE.set(fetched);
    ETH_ORACLE_CACHE.get().copied().flatten()
}

/// Chain context the fee calculation needs. A single CLI invocation runs
/// against exactly one chain (from `RPC_URL`), so this is safe to store once
/// after the RPC is probed. Stored globally so `initial_transaction_fees`
/// (a synchronous function called from many places) can resolve the per-chain
/// gas level and real values without threading `chain_id` through every call
/// site.
#[derive(Clone, Copy, Debug)]
pub struct GasFeeState {
    pub chain_id: u64,
    pub eth_oracle: Option<EthGasOracle>,
}

impl Default for GasFeeState {
    fn default() -> Self {
        Self {
            chain_id: 0,
            eth_oracle: None,
        }
    }
}

static GAS_FEE_STATE: OnceLock<GasFeeState> = OnceLock::new();

/// Initialize the process-wide gas-fee state with the probed chain id,
/// fetching the Etherscan oracle first for Ethereum mainnet. Called once right
/// after the RPC is probed (async entry points). Safe to call more than once —
/// the first value wins.
pub async fn initialize_gas_fee_state(chain_id: u64) {
    let eth_oracle = if chain_id == 1 {
        eth_gas_oracle().await
    } else {
        None
    };
    let _ = GAS_FEE_STATE.set(GasFeeState {
        chain_id,
        eth_oracle,
    });
}

/// The current process-wide gas-fee state. Defaults to `chain_id = 0` with no
/// oracle when not yet initialized (e.g. unit tests) — which resolves to the
/// medium default and the RPC's own estimate.
pub fn current_gas_fee_state() -> GasFeeState {
    GAS_FEE_STATE.get().copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::GasFeeLevel;

    #[test]
    fn gwei_conversion_rounds_up() {
        assert_eq!(gwei_f64_to_wei(0.153), U256::from(153_000_000));
        assert_eq!(gwei_f64_to_wei(1.0), U256::from(1_000_000_000));
        // 0.0000000001 gwei rounds up to 1 wei.
        assert_eq!(gwei_f64_to_wei(0.0000000001), U256::from(1));
    }

    #[test]
    fn tier_selects_the_matching_etherscan_value() {
        let oracle = EthGasOracle {
            safe_gas_price_gwei: 0.153,
            propose_gas_price_gwei: 0.154,
            fast_gas_price_gwei: 0.253,
        };
        assert_eq!(oracle.tier(GasFeeLevel::Slow), 0.153);
        assert_eq!(oracle.tier(GasFeeLevel::Medium), 0.154);
        assert_eq!(oracle.tier(GasFeeLevel::Fast), 0.253);
    }
}
