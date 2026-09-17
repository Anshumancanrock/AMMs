use anchor_lang::prelude::*;

#[constant]
pub const CONFIG_SEED: &[u8] = b"config";

#[constant]
pub const LP_MINT_SEED: &[u8] = b"lp_mint";

#[constant]
pub const TREASURY_SEED: &[u8] = b"treasury";

/// 10_000 bps = 100%.
pub const BPS_DENOMINATOR: u64 = 10_000;

/// Ceiling on the swap fee, 10%.
pub const MAX_FEE_BPS: u16 = 1_000;

/// LP mint decimals, independent of the pool mints.
pub const LP_DECIMALS: u8 = 6;
