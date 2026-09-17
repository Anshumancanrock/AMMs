//! Constant product AMM: two vaults, an LP mint, and a swap fee split between
//! the liquidity providers and a protocol treasury.
//!
//! Curve math lives in [`curve`] instead of a dependency, so the rounding and
//! overflow behaviour behind every instruction is auditable here.

pub mod error;
pub mod state;

use anchor_lang::prelude::*;

pub use state::*;

declare_id!("859An1PzpQeQQdfXbaq1vpC7zqVmy4tVrg6K1pThzBSY");

#[program]
pub mod amm {}
