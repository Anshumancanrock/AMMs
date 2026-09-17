//! Constant product curve math. Invariant: `x * y = k`.
//!
//! Integer only. Every product widens to `u128` before dividing, so two
//! `u64::MAX` reserves cannot overflow the intermediate, and narrows back
//! through a checked conversion.
//!
//! Rounding direction is deliberate throughout: fees and required deposits
//! round up, payouts round down. The reverse lets a loop of dust-sized
//! operations bleed the reserves one base unit at a time.

use anchor_lang::prelude::*;

use crate::{constants::*, error::AmmError};

/// What a swap moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapAmounts {
    pub amount_out: u64,
    /// Part of the fee that stays in the vault for the liquidity providers.
    pub lp_fee: u64,
    /// Part of the fee that goes to the treasury.
    pub protocol_fee: u64,
}

/// `a * b / denominator` on a `u128` intermediate.
fn mul_div(a: u64, b: u64, denominator: u64, round_up: bool) -> Result<u64> {
    require!(denominator != 0, AmmError::MathOverflow);

    let numerator = (a as u128) * (b as u128);
    let denominator = denominator as u128;
    let mut result = numerator / denominator;
    if round_up && numerator % denominator != 0 {
        result += 1;
    }

    u64::try_from(result).map_err(|_| AmmError::MathOverflow.into())
}

/// Integer square root by Newton's method.
fn integer_sqrt(value: u128) -> u128 {
    if value < 2 {
        return value;
    }

    let mut guess = value;
    let mut next = value.div_ceil(2);
    while next < guess {
        guess = next;
        next = (guess + value / guess) / 2;
    }
    guess
}

/// Swap output and the fee split.
///
/// The fee comes off the input, rounded up. What remains is priced against
/// the invariant, rounded down:
///
/// ```text
/// amount_out = reserve_out * amount_in_net / (reserve_in + amount_in_net)
/// ```
///
/// Only the protocol share leaves the pool, so `k` never falls.
pub fn compute_swap(
    reserve_in: u64,
    reserve_out: u64,
    amount_in: u64,
    fee_bps: u16,
    protocol_fee_bps: u16,
) -> Result<SwapAmounts> {
    require!(amount_in > 0, AmmError::ZeroAmount);
    require!(reserve_in > 0 && reserve_out > 0, AmmError::PoolEmpty);

    let fee = mul_div(amount_in, fee_bps as u64, BPS_DENOMINATOR, true)?;
    let protocol_fee = mul_div(fee, protocol_fee_bps as u64, BPS_DENOMINATOR, false)?;
    let amount_in_net = amount_in.checked_sub(fee).ok_or(AmmError::MathOverflow)?;
    require!(amount_in_net > 0, AmmError::ZeroAmount);

    let denominator = reserve_in
        .checked_add(amount_in_net)
        .ok_or(AmmError::MathOverflow)?;
    let amount_out = mul_div(reserve_out, amount_in_net, denominator, false)?;
    require!(amount_out > 0, AmmError::ZeroAmount);

    Ok(SwapAmounts {
        amount_out,
        lp_fee: fee - protocol_fee,
        protocol_fee,
    })
}
