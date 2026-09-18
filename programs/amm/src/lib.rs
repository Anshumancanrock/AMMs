//! Constant product AMM: two vaults, an LP mint, and a swap fee split between
//! the liquidity providers and a protocol treasury.
//!
//! Curve math lives in [`curve`] instead of a dependency, so the rounding and
//! overflow behaviour behind every instruction is auditable here.

pub mod constants;
pub mod curve;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("859An1PzpQeQQdfXbaq1vpC7zqVmy4tVrg6K1pThzBSY");

#[program]
pub mod amm {
    use super::*;

    /// Creates a pool for `mint_x` and `mint_y`. `seed` namespaces it so one
    /// pair can have several. `fee_bps` is charged on swap input and
    /// `protocol_fee_bps` is the treasury's share of that fee.
    pub fn initialize(
        ctx: Context<Initialize>,
        seed: u64,
        fee_bps: u16,
        protocol_fee_bps: u16,
        authority: Pubkey,
    ) -> Result<()> {
        ctx.accounts
            .handler(seed, fee_bps, protocol_fee_bps, authority, &ctx.bumps)
    }
}
