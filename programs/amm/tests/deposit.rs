mod common;

use {amm::error::AmmError, common::*};

/// A 1:4 pool. `sqrt(1e11 * 4e11)` is 2e11, so the first deposit mints
/// 200_000_000_000 LP tokens, 1_000 of which stay locked in the pool.
const DEPOSIT_X: u64 = 100_000_000_000;
const DEPOSIT_Y: u64 = 400_000_000_000;
const FIRST_LP_SUPPLY: u64 = 200_000_000_000;

#[test]
fn first_deposit_sets_the_price_and_locks_the_minimum_liquidity() {
    let mut pool = Pool::opened();
    let lp = pool.lp();

    pool.deposit(&lp, DEPOSIT_X, DEPOSIT_Y, 0).unwrap();

    assert_eq!(pool.balance(&pool.vault_x), DEPOSIT_X);
    assert_eq!(pool.balance(&pool.vault_y), DEPOSIT_Y);
    assert_eq!(pool.lp_supply(), FIRST_LP_SUPPLY);
    assert_eq!(
        pool.balance(&pool.locked_lp),
        amm::constants::MINIMUM_LIQUIDITY
    );
    assert_eq!(
        pool.balance(&pool.ata(&lp.pubkey(), &pool.lp_mint)),
        FIRST_LP_SUPPLY - amm::constants::MINIMUM_LIQUIDITY
    );
}

#[test]
fn later_deposits_follow_the_pool_ratio_and_ignore_the_surplus() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();
    let before_x = pool.balance(&pool.ata(&trader.pubkey(), &pool.mint_x));
    let before_y = pool.balance(&pool.ata(&trader.pubkey(), &pool.mint_y));

    // Offers 10_000_000_000 x and 100_000_000_000 y into a 1:4 pool, so the x
    // side is the binding one and only a quarter of the y is taken.
    pool.deposit(&trader, 10_000_000_000, 100_000_000_000, 0)
        .unwrap();

    let spent_x = before_x - pool.balance(&pool.ata(&trader.pubkey(), &pool.mint_x));
    let spent_y = before_y - pool.balance(&pool.ata(&trader.pubkey(), &pool.mint_y));
    assert_eq!(spent_x, 10_000_000_000);
    assert_eq!(spent_y, 40_000_000_000);
    assert_eq!(
        pool.balance(&pool.ata(&trader.pubkey(), &pool.lp_mint)),
        20_000_000_000
    );
    assert_eq!(pool.lp_supply(), FIRST_LP_SUPPLY + 20_000_000_000);
}

#[test]
fn deposits_keep_the_lp_supply_backed_by_the_vaults() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();

    for _ in 0..5 {
        pool.deposit(&trader, 3_333_333_333, 77_777_777_777, 0)
            .unwrap();
    }

    // The LP supply never runs ahead of the geometric mean of the vaults, so
    // no deposit can dilute the tokens that were minted before it.
    let supply = pool.lp_supply() as u128;
    assert!(supply * supply <= pool.invariant());
}

#[test]
fn the_pool_reopens_after_everything_withdrawable_is_withdrawn() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let lp = pool.lp();
    let held = pool.balance(&pool.ata(&lp.pubkey(), &pool.lp_mint));

    pool.withdraw(&lp, held, 0, 0).unwrap();
    assert_eq!(pool.lp_supply(), amm::constants::MINIMUM_LIQUIDITY);

    // The locked tokens keep the supply above zero, so the next deposit takes
    // the proportional path against the dust left in the vaults rather than
    // trying to set a new price.
    pool.deposit(&lp, DEPOSIT_X, DEPOSIT_Y, 0).unwrap();

    assert!(pool.lp_supply() > amm::constants::MINIMUM_LIQUIDITY);
    assert!(pool.balance(&pool.vault_x) > 0);
    assert!(pool.balance(&pool.vault_y) > 0);
    let held_again = pool.balance(&pool.ata(&lp.pubkey(), &pool.lp_mint));
    pool.withdraw(&lp, held_again, 0, 0).unwrap();
}

#[test]
fn a_donation_straight_into_a_vault_cannot_break_the_pool() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let lp = pool.lp();
    let trader = pool.trader();
    let supply_before = pool.lp_supply();

    let (mint_x, vault_x) = (pool.mint_x, pool.vault_x);
    pool.donate(&trader, &mint_x, &vault_x, 50_000_000_000);

    // Nothing was minted for it, so the donation belongs to the existing LP
    // tokens, and every instruction still works off the real balances.
    assert_eq!(pool.lp_supply(), supply_before);
    pool.deposit(&trader, 1_000_000_000, 10_000_000_000, 0)
        .unwrap();
    pool.swap(&trader, pool.mint_y, pool.mint_x, 1_000_000_000, 0)
        .unwrap();
    pool.withdraw(&lp, 1_000_000_000, 0, 0).unwrap();

    let supply = pool.lp_supply() as u128;
    assert!(supply * supply <= pool.invariant());
}

#[test]
fn rejects_a_first_deposit_below_the_locked_minimum() {
    let mut pool = Pool::opened();
    let lp = pool.lp();

    assert_amm_error(
        pool.deposit(&lp, 10, 10, 0),
        AmmError::InsufficientInitialLiquidity,
    );
}

#[test]
fn rejects_a_deposit_that_mints_fewer_lp_tokens_than_asked_for() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();

    assert_amm_error(
        pool.deposit(&trader, 10_000_000_000, 100_000_000_000, 20_000_000_001),
        AmmError::SlippageExceeded,
    );
}

#[test]
fn rejects_a_deposit_of_nothing() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();

    assert_amm_error(pool.deposit(&trader, 0, 1_000, 0), AmmError::ZeroAmount);
}

#[test]
fn rejects_a_deposit_while_the_pool_is_locked() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let admin = pool.admin();
    let trader = pool.trader();
    pool.set_locked(&admin, true).unwrap();

    assert_amm_error(
        pool.deposit(&trader, 1_000_000, 4_000_000, 0),
        AmmError::PoolLocked,
    );
}
