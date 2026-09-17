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

/// What a deposit moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepositAmounts {
    pub amount_x: u64,
    pub amount_y: u64,
    /// The depositor's share.
    pub lp_tokens: u64,
}

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

/// Deposit amounts and the LP tokens they mint.
///
/// A first deposit sets the price and mints `sqrt(x * y)`. After that the
/// smaller side decides the LP amount and
/// the other side is trimmed to the pool ratio. The trim rounds up, so a
/// depositor can pay one base unit over the ratio but never one under.
pub fn compute_deposit(
    reserve_x: u64,
    reserve_y: u64,
    lp_supply: u64,
    max_x: u64,
    max_y: u64,
) -> Result<DepositAmounts> {
    require!(max_x > 0 && max_y > 0, AmmError::ZeroAmount);

    if lp_supply == 0 {
        let lp_tokens = u64::try_from(integer_sqrt((max_x as u128) * (max_y as u128)))
            .map_err(|_| AmmError::MathOverflow)?;
        require!(lp_tokens > 0, AmmError::ZeroAmount);

        return Ok(DepositAmounts {
            amount_x: max_x,
            amount_y: max_y,
            lp_tokens,
        });
    }

    require!(reserve_x > 0 && reserve_y > 0, AmmError::PoolEmpty);

    let lp_from_x = mul_div(max_x, lp_supply, reserve_x, false)?;
    let lp_from_y = mul_div(max_y, lp_supply, reserve_y, false)?;
    let lp_tokens = lp_from_x.min(lp_from_y);
    require!(lp_tokens > 0, AmmError::ZeroAmount);

    Ok(DepositAmounts {
        amount_x: mul_div(lp_tokens, reserve_x, lp_supply, true)?,
        amount_y: mul_div(lp_tokens, reserve_y, lp_supply, true)?,
        lp_tokens,
    })
}

/// Tokens released for burning `lp_amount`. Both sides round down, so an
/// exit never takes more than its share.
pub fn compute_withdraw(
    reserve_x: u64,
    reserve_y: u64,
    lp_supply: u64,
    lp_amount: u64,
) -> Result<(u64, u64)> {
    require!(lp_amount > 0, AmmError::ZeroAmount);
    require!(lp_supply > 0, AmmError::PoolEmpty);
    require!(lp_amount <= lp_supply, AmmError::MathOverflow);

    let amount_x = mul_div(lp_amount, reserve_x, lp_supply, false)?;
    let amount_y = mul_div(lp_amount, reserve_y, lp_supply, false)?;
    require!(amount_x > 0 && amount_y > 0, AmmError::ZeroAmount);

    Ok((amount_x, amount_y))
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
