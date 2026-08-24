use alloy_primitives::U256;
use thiserror::Error;

const BASIS_POINTS: u32 = 10_000;

/// Gas fee aggressiveness level. The actual fee values are real per-chain
/// values fetched at runtime (Etherscan gas tracker for Ethereum, the RPC's
/// own fee estimate for cheap chains) — never an invented multiplier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GasFeeLevel {
    Slow,
    Medium,
    Fast,
}

impl GasFeeLevel {
    /// Chain-specific default level. Fast on the chains the user races drops
    /// on (Robinhood, Ink), slow on Ethereum mainnet where gas is expensive,
    /// medium everywhere else.
    pub fn default_for_chain(chain_id: u64) -> Self {
        match chain_id {
            4663 | 57073 => GasFeeLevel::Fast,
            1 => GasFeeLevel::Slow,
            _ => GasFeeLevel::Medium,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "slow" => Some(GasFeeLevel::Slow),
            "medium" => Some(GasFeeLevel::Medium),
            "fast" => Some(GasFeeLevel::Fast),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GasFeeLevel::Slow => "slow",
            GasFeeLevel::Medium => "medium",
            GasFeeLevel::Fast => "fast",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Eip1559Fees {
    pub max_fee_per_gas: U256,
    pub max_priority_fee_per_gas: U256,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutomaticFeePolicy {
    multiplier_bps: u32,
    replacement_bump_bps: u32,
}

impl AutomaticFeePolicy {
    pub fn new(multiplier_bps: u32, replacement_bump_bps: u32) -> Result<Self, FeeError> {
        if multiplier_bps < BASIS_POINTS || replacement_bump_bps <= BASIS_POINTS {
            return Err(FeeError::InvalidMultiplier);
        }
        Ok(Self {
            multiplier_bps,
            replacement_bump_bps,
        })
    }

    pub fn initial(self, estimate: Eip1559Fees) -> Result<Eip1559Fees, FeeError> {
        multiply_fees(estimate, self.multiplier_bps)
    }

    pub fn replacement(self, pending: Eip1559Fees) -> Result<Eip1559Fees, FeeError> {
        multiply_fees(pending, self.replacement_bump_bps)
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum FeeError {
    #[error("fee multipliers must not reduce fees and replacements must increase them")]
    InvalidMultiplier,
    #[error("fee calculation overflowed")]
    Overflow,
}

fn multiply_fees(fees: Eip1559Fees, basis_points: u32) -> Result<Eip1559Fees, FeeError> {
    Ok(Eip1559Fees {
        max_fee_per_gas: multiply_ceil(fees.max_fee_per_gas, basis_points)?,
        max_priority_fee_per_gas: multiply_ceil(fees.max_priority_fee_per_gas, basis_points)?,
    })
}

fn multiply_ceil(value: U256, basis_points: u32) -> Result<U256, FeeError> {
    let numerator = value
        .checked_mul(U256::from(basis_points))
        .and_then(|scaled| scaled.checked_add(U256::from(BASIS_POINTS - 1)))
        .ok_or(FeeError::Overflow)?;
    Ok(numerator / U256::from(BASIS_POINTS))
}
