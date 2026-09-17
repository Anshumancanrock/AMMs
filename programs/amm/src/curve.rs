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

use crate::error::AmmError;

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
