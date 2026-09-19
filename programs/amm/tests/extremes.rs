//! Extreme reserve sizes: a pool holding a quarter of the `u64` range, and
//! one holding barely more than the locked minimum.

mod common;

use {amm::error::AmmError, common::*};

#[test]
fn a_pool_holding_a_quarter_of_the_u64_range_still_adds_up() {
    let mut pool = Pool::opened();
    let lp = pool.lp();
    let trader = pool.trader();
    let whale = u64::MAX / 4;

    let (mint_x, mint_y) = (pool.mint_x, pool.mint_y);
    for (wallet, mint) in [
        (lp.pubkey(), mint_x),
        (lp.pubkey(), mint_y),
        (trader.pubkey(), mint_x),
    ] {
        let account = pool.ata(&wallet, &mint);
        pool.mint_tokens(&mint, &account, whale - STARTING_BALANCE);
    }

    pool.deposit(&lp, whale, whale, 0).unwrap();
    assert_eq!(pool.lp_supply(), whale);

    // Numerators around 4.6e36, well inside the curve's u128 intermediates.
    pool.swap(&trader, mint_x, mint_y, 1_000_000_000_000_000_000, 0)
        .unwrap();
    assert!(pool.invariant() > (whale as u128) * (whale as u128));

    let held = pool.balance(&pool.ata(&lp.pubkey(), &pool.lp_mint));
    pool.withdraw(&lp, held, 0, 0).unwrap();
    assert_eq!(pool.lp_supply(), amm::constants::MINIMUM_LIQUIDITY);
}

#[test]
fn a_pool_barely_above_the_locked_minimum_still_trades() {
    let mut pool = Pool::opened();
    let lp = pool.lp();
    let trader = pool.trader();

    // 2_000 of each mints 2_000 LP tokens, half of them locked.
    pool.deposit(&lp, 2_000, 2_000, 0).unwrap();
    assert_eq!(pool.lp_supply(), 2_000);

    // fee = ceil(1_000 * 30 / 10_000) = 3, net = 997,
    // out = floor(2_000 * 997 / 2_997) = 665.
    pool.swap(&trader, pool.mint_x, pool.mint_y, 1_000, 0)
        .unwrap();
    assert_eq!(pool.balance(&pool.vault_x), 3_000);
    assert_eq!(pool.balance(&pool.vault_y), 1_335);
    // A fifth of a three unit fee rounds to nothing, so the pool keeps it all.
    assert_eq!(pool.balance(&pool.treasury_x), 0);

    pool.withdraw(&lp, 1_000, 0, 0).unwrap();
    assert_eq!(pool.lp_supply(), amm::constants::MINIMUM_LIQUIDITY);
}

#[test]
fn a_burn_that_would_pay_nothing_on_one_side_is_refused() {
    let mut pool = Pool::opened();
    let lp = pool.lp();
    let trader = pool.trader();

    // The smallest pool the program allows leaves this provider one LP token.
    pool.deposit(&lp, 1_001, 1_001, 0).unwrap();
    pool.swap(&trader, pool.mint_x, pool.mint_y, 1_000, 0)
        .unwrap();

    // One token out of 1_001 is worth one unit of x and zero of y, so the
    // burn is refused rather than paid on one side only. Same rule as
    // Uniswap V2's INSUFFICIENT_LIQUIDITY_BURNED.
    assert_amm_error(pool.withdraw(&lp, 1, 0, 0), AmmError::ZeroAmount);
}
