use crate::{
    chain::FeeEstimate,
    config::{FeeMode, FeesConfig},
    domain::{Eip1559Fees, FeeError, GasFeeLevel},
    gas_oracle::{GasFeeState, current_gas_fee_state, gwei_f64_to_wei},
};

const ETHEREUM_MAINNET_CHAIN_ID: u64 = 1;

/// Compute the initial transaction fees.
///
/// In automatic mode the chain's gas level (from `.env`, or the per-chain
/// default) selects the real gas values:
/// - Ethereum mainnet uses the real slow/medium/fast prices from Etherscan's
///   gas tracker (the exact values shown on the explorer).
/// - Cheap chains (Robinhood, Ink, ...) have no slow/medium/fast breakdown on
///   their explorers, so the RPC's own real fee estimate is used verbatim.
///
/// No invented multipliers anywhere — the values are the real ones for the
/// selected level.
pub(crate) fn initial_transaction_fees(
    config: FeesConfig,
    estimate: FeeEstimate,
) -> Result<Eip1559Fees, FeeError> {
    let state = current_gas_fee_state();
    resolve_initial_fees(config, state, estimate)
}

/// Pure, testable fee resolution given an explicit gas-fee state.
pub(crate) fn resolve_initial_fees(
    config: FeesConfig,
    state: GasFeeState,
    estimate: FeeEstimate,
) -> Result<Eip1559Fees, FeeError> {
    match config.mode {
        FeeMode::Automatic => {
            let level = config
                .gas_fee_level
                .unwrap_or_else(|| GasFeeLevel::default_for_chain(state.chain_id));

            if state.chain_id == ETHEREUM_MAINNET_CHAIN_ID {
                if let Some(oracle) = state.eth_oracle {
                    // Etherscan tiers are total gas prices (base + tip), so the
                    // priority fee is the tier minus the current base fee.
                    let max_fee_per_gas = gwei_f64_to_wei(oracle.tier(level));
                    let base_fee = estimate
                        .max_fee_per_gas
                        .saturating_sub(estimate.max_priority_fee_per_gas);
                    let max_priority_fee_per_gas = max_fee_per_gas
                        .saturating_sub(base_fee)
                        .max(alloy_primitives::U256::from(1));
                    return Ok(Eip1559Fees {
                        max_fee_per_gas,
                        max_priority_fee_per_gas,
                    });
                }
            }

            // Fallback (oracle unavailable, or non-ETH chain): the RPC's real
            // estimate as-is.
            Ok(Eip1559Fees {
                max_fee_per_gas: estimate.max_fee_per_gas,
                max_priority_fee_per_gas: estimate.max_priority_fee_per_gas,
            })
        }
        FeeMode::Manual {
            max_fee_per_gas,
            max_priority_fee_per_gas,
        } => Ok(Eip1559Fees {
            max_fee_per_gas,
            max_priority_fee_per_gas,
        }),
    }
}

pub(crate) fn maximum_transaction_fees(
    config: FeesConfig,
    maximum_attempts: u32,
    mut fees: Eip1559Fees,
) -> Result<Eip1559Fees, FeeError> {
    let replacement = crate::domain::AutomaticFeePolicy::new(10_000, config.replacement_bump_bps)?;
    for _ in 1..maximum_attempts {
        fees = replacement.replacement(fees)?;
    }
    Ok(fees)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::U256;
    use crate::gas_oracle::EthGasOracle;

    fn automatic_config() -> FeesConfig {
        FeesConfig {
            mode: FeeMode::Automatic,
            replacement_bump_bps: 11_250,
            gas_fee_level: None,
        }
    }

    fn rh_state() -> GasFeeState {
        GasFeeState {
            chain_id: 4663,
            eth_oracle: None,
        }
    }

    #[test]
    fn non_ethereum_uses_the_real_rpc_estimate_as_is() {
        // Robinhood has no slow/medium/fast on its explorer, so the RPC's real
        // estimate is used verbatim — no multiplier invented.
        let config = automatic_config();
        let estimate = FeeEstimate {
            max_fee_per_gas: U256::from(20_000_000), // 0.02 gwei
            max_priority_fee_per_gas: U256::from(0),
        };
        let fees = resolve_initial_fees(config, rh_state(), estimate).expect("fees");
        assert_eq!(fees.max_fee_per_gas, estimate.max_fee_per_gas);
        assert_eq!(fees.max_priority_fee_per_gas, estimate.max_priority_fee_per_gas);
    }

    #[test]
    fn ethereum_uses_the_real_etherscan_value_for_the_selected_level() {
        let config = automatic_config();
        let state = GasFeeState {
            chain_id: 1,
            eth_oracle: Some(EthGasOracle {
                safe_gas_price_gwei: 0.153,
                propose_gas_price_gwei: 0.154,
                fast_gas_price_gwei: 0.253,
            }),
        };
        let estimate = FeeEstimate {
            max_fee_per_gas: U256::from(162_000_000), // base ~0.162 gwei
            max_priority_fee_per_gas: U256::from(1_000_000),
        };
        // Default for Ethereum is slow → Etherscan SafeGasPrice 0.153 gwei.
        let fees = resolve_initial_fees(config, state, estimate).expect("fees");
        assert_eq!(fees.max_fee_per_gas, U256::from(153_000_000));
        // priority = 0.153 gwei - base (0.162-0.001) clamped to >= 1 wei
        assert!(fees.max_priority_fee_per_gas >= U256::from(1));
        assert_eq!(fees.max_fee_per_gas, U256::from(153_000_000));
    }

    #[test]
    fn explicit_level_overrides_the_chain_default() {
        let mut config = automatic_config();
        config.gas_fee_level = Some(GasFeeLevel::Fast);
        let state = GasFeeState {
            chain_id: 1,
            eth_oracle: Some(EthGasOracle {
                safe_gas_price_gwei: 0.153,
                propose_gas_price_gwei: 0.154,
                fast_gas_price_gwei: 0.253,
            }),
        };
        let estimate = FeeEstimate {
            max_fee_per_gas: U256::from(162_000_000),
            max_priority_fee_per_gas: U256::from(1_000_000),
        };
        let fees = resolve_initial_fees(config, state, estimate).expect("fees");
        assert_eq!(fees.max_fee_per_gas, U256::from(253_000_000)); // Etherscan Fast
    }

    #[test]
    fn manual_mode_returns_the_configured_values() {
        let config = FeesConfig {
            mode: FeeMode::Manual {
                max_fee_per_gas: U256::from(5_000_000_000_u64),
                max_priority_fee_per_gas: U256::from(1_000_000_000_u64),
            },
            replacement_bump_bps: 11_250,
            gas_fee_level: None,
        };
        let estimate = FeeEstimate {
            max_fee_per_gas: U256::from(1),
            max_priority_fee_per_gas: U256::from(1),
        };
        let fees = resolve_initial_fees(config, rh_state(), estimate).expect("fees");
        assert_eq!(fees.max_fee_per_gas, U256::from(5_000_000_000_u64));
        assert_eq!(fees.max_priority_fee_per_gas, U256::from(1_000_000_000_u64));
    }

    #[test]
    fn replacement_bump_increases_fees() {
        let config = automatic_config();
        let initial = Eip1559Fees {
            max_fee_per_gas: U256::from(125),
            max_priority_fee_per_gas: U256::from(5),
        };
        let maximum = maximum_transaction_fees(config, 3, initial).expect("maximum fees");
        assert!(maximum.max_fee_per_gas > initial.max_fee_per_gas);
        assert!(maximum.max_priority_fee_per_gas > initial.max_priority_fee_per_gas);
    }
}
