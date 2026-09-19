use anchor_lang::{prelude::*, solana_program::program_pack::Pack};
use anchor_spl::token::spl_token::state::Mint;

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

/// Minted to a pool-owned account on the first deposit and never moved. LP
/// supply therefore never returns to zero, which is what prices a donation
/// attack on the share price out of reach.
pub const MINIMUM_LIQUIDITY: u64 = 1_000;

/// A mint with no extensions is exactly this long under either token
/// program. Anything longer carries Token-2022 extensions.
pub const PLAIN_MINT_LEN: usize = Mint::LEN;

/// LP mint decimals, independent of the pool mints.
pub const LP_DECIMALS: u8 = 6;
