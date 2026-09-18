//! Constant product AMM: two vaults, an LP mint, and a swap fee split between
//! the liquidity providers and a protocol treasury.
//!
//! Curve math lives in [`curve`] instead of a dependency, so the rounding and
//! overflow behaviour behind every instruction is auditable here.

pub mod constants;
pub mod curve;
pub mod error;
pub mod events;
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

    /// Adds liquidity at the current ratio, up to `max_x` and `max_y`.
    /// Reverts below `min_lp`.
    pub fn deposit(ctx: Context<Deposit>, max_x: u64, max_y: u64, min_lp: u64) -> Result<()> {
        ctx.accounts.handler(max_x, max_y, min_lp)
    }

    /// Burns `lp_tokens` and pays out both sides. Reverts below `min_x` or
    /// `min_y`.
    pub fn withdraw(ctx: Context<Withdraw>, lp_tokens: u64, min_x: u64, min_y: u64) -> Result<()> {
        ctx.accounts.handler(lp_tokens, min_x, min_y)
    }

    /// Trades `amount_in` of `mint_in` for at least `min_amount_out` of
    /// `mint_out`.
    pub fn swap(ctx: Context<Swap>, amount_in: u64, min_amount_out: u64) -> Result<()> {
        ctx.accounts.handler(amount_in, min_amount_out)
    }
}
