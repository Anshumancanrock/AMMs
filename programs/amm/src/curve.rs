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
    /// The depositor's share. A first deposit mints `MINIMUM_LIQUIDITY` on
    /// top of this and locks it.
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
/// A first deposit sets the price and mints `sqrt(x * y)` less the locked
/// `MINIMUM_LIQUIDITY`. After that the smaller side decides the LP amount and
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
        let minted = integer_sqrt((max_x as u128) * (max_y as u128));
        let minted = u64::try_from(minted).map_err(|_| AmmError::MathOverflow)?;
        let lp_tokens = minted
            .checked_sub(MINIMUM_LIQUIDITY)
            .ok_or(AmmError::InsufficientInitialLiquidity)?;
        require!(lp_tokens > 0, AmmError::InsufficientInitialLiquidity);

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

#[cfg(test)]
mod tests {
    use super::*;

    const FEE_BPS: u16 = 30;
    const PROTOCOL_FEE_BPS: u16 = 2_000;

    /// `k` after a swap, with the protocol fee taken out of the vault.
    fn invariant_after_swap(
        reserve_in: u64,
        reserve_out: u64,
        amount_in: u64,
        swap: SwapAmounts,
    ) -> u128 {
        let new_in = (reserve_in + amount_in - swap.protocol_fee) as u128;
        let new_out = (reserve_out - swap.amount_out) as u128;
        new_in * new_out
    }

    #[test]
    fn integer_sqrt_is_exact_on_squares_and_floors_otherwise() {
        assert_eq!(integer_sqrt(0), 0);
        assert_eq!(integer_sqrt(1), 1);
        assert_eq!(integer_sqrt(2), 1);
        assert_eq!(integer_sqrt(1_000_000), 1_000);
        assert_eq!(integer_sqrt(1_000_001), 1_000);
        assert_eq!(integer_sqrt(u128::from(u64::MAX)), 4_294_967_295);
    }

    #[test]
    fn first_deposit_mints_sqrt_of_the_product_minus_locked_liquidity() {
        let deposit = compute_deposit(0, 0, 0, 1_000_000, 4_000_000).unwrap();

        assert_eq!(deposit.amount_x, 1_000_000);
        assert_eq!(deposit.amount_y, 4_000_000);
        assert_eq!(deposit.lp_tokens, 2_000_000 - MINIMUM_LIQUIDITY);
    }

    #[test]
    fn first_deposit_below_the_locked_amount_is_rejected() {
        assert!(compute_deposit(0, 0, 0, 10, 10).is_err());
    }

    #[test]
    fn later_deposits_follow_the_smaller_side() {
        // Pool at 1:4, the depositor offers too much y.
        let deposit = compute_deposit(1_000_000, 4_000_000, 2_000_000, 100_000, 900_000).unwrap();

        assert_eq!(deposit.lp_tokens, 200_000);
        assert_eq!(deposit.amount_x, 100_000);
        assert_eq!(deposit.amount_y, 400_000);
    }

    #[test]
    fn deposit_never_asks_for_more_than_the_caller_offered() {
        let (reserve_x, reserve_y, supply) = (1_000_003u64, 7_777_771u64, 2_645_749u64);

        for offered in 1..500u64 {
            let max_x = offered * 37;
            let max_y = offered * 211;
            if let Ok(deposit) = compute_deposit(reserve_x, reserve_y, supply, max_x, max_y) {
                assert!(deposit.amount_x <= max_x);
                assert!(deposit.amount_y <= max_y);
            }
        }
    }

    #[test]
    fn deposit_then_withdraw_never_returns_more_than_it_put_in() {
        let (mut reserve_x, mut reserve_y, mut supply) = (1_000_003u64, 7_777_771u64, 2_645_749u64);

        for step in 1..200u64 {
            let max_x = step * 13;
            let max_y = step * 101;
            let deposit = compute_deposit(reserve_x, reserve_y, supply, max_x, max_y).unwrap();
            reserve_x += deposit.amount_x;
            reserve_y += deposit.amount_y;
            supply += deposit.lp_tokens;

            let (out_x, out_y) =
                compute_withdraw(reserve_x, reserve_y, supply, deposit.lp_tokens).unwrap();
            assert!(out_x <= deposit.amount_x);
            assert!(out_y <= deposit.amount_y);

            reserve_x -= out_x;
            reserve_y -= out_y;
            supply -= deposit.lp_tokens;
        }
    }

    #[test]
    fn swap_prices_against_the_invariant() {
        // 1_000_000 / 1_000_000 pool, 1_000 in, 0.3% fee.
        let swap = compute_swap(1_000_000, 1_000_000, 1_000, FEE_BPS, PROTOCOL_FEE_BPS).unwrap();

        assert_eq!(swap.lp_fee + swap.protocol_fee, 3);
        assert_eq!(swap.protocol_fee, 0); // 20% of 3 base units rounds down to 0
        assert_eq!(swap.amount_out, 996);
    }

    #[test]
    fn swap_splits_the_fee_between_the_pool_and_the_treasury() {
        let swap =
            compute_swap(50_000_000, 50_000_000, 1_000_000, FEE_BPS, PROTOCOL_FEE_BPS).unwrap();

        assert_eq!(swap.lp_fee + swap.protocol_fee, 3_000);
        assert_eq!(swap.protocol_fee, 600);
        assert_eq!(swap.lp_fee, 2_400);
    }

    #[test]
    fn swap_never_lowers_the_invariant() {
        let reserve_in = 923_457_011u64;
        let reserve_out = 51_112_887u64;
        let before = (reserve_in as u128) * (reserve_out as u128);

        for amount_in in [100u64, 1_000, 65_537, 10_000_000, 500_000_000] {
            let swap = compute_swap(
                reserve_in,
                reserve_out,
                amount_in,
                FEE_BPS,
                PROTOCOL_FEE_BPS,
            )
            .unwrap();
            assert!(swap.amount_out < reserve_out);
            assert!(invariant_after_swap(reserve_in, reserve_out, amount_in, swap) >= before);
        }
    }

    #[test]
    fn swap_holds_the_invariant_even_when_the_treasury_takes_the_whole_fee() {
        let (reserve_in, reserve_out) = (923_457_011u64, 51_112_887u64);
        let before = (reserve_in as u128) * (reserve_out as u128);

        for amount_in in [100u64, 1_000, 65_537, 10_000_000, 500_000_000] {
            let swap = compute_swap(reserve_in, reserve_out, amount_in, FEE_BPS, 10_000).unwrap();
            assert_eq!(swap.lp_fee, 0);
            assert!(invariant_after_swap(reserve_in, reserve_out, amount_in, swap) >= before);
        }
    }

    #[test]
    fn swap_holds_up_at_the_top_of_the_u64_range() {
        let swap = compute_swap(u64::MAX / 2, u64::MAX / 2, u64::MAX / 4, FEE_BPS, 0).unwrap();

        assert!(swap.amount_out > 0);
        assert!(swap.amount_out < u64::MAX / 2);
    }

    #[test]
    fn swap_too_small_to_move_the_price_is_rejected() {
        // Against a pool this lopsided the output rounds down to zero.
        assert!(compute_swap(1_000_000_000, 1, 1_000, FEE_BPS, PROTOCOL_FEE_BPS).is_err());
        // And an input the fee swallows whole leaves nothing to price.
        assert!(compute_swap(1_000_000, 1_000_000, 1, FEE_BPS, PROTOCOL_FEE_BPS).is_err());
    }

    #[test]
    fn empty_pool_cannot_be_swapped_against() {
        assert!(compute_swap(0, 1_000, 100, FEE_BPS, PROTOCOL_FEE_BPS).is_err());
        assert!(compute_swap(1_000, 0, 100, FEE_BPS, PROTOCOL_FEE_BPS).is_err());
    }
}
