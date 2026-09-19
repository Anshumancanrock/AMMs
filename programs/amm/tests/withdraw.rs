mod common;

use {amm::error::AmmError, common::*};

const DEPOSIT_X: u64 = 100_000_000_000;
const DEPOSIT_Y: u64 = 400_000_000_000;
/// LP tokens the first depositor receives: `sqrt(1e11 * 4e11) - 1_000`.
const LP_TOKENS: u64 = 200_000_000_000 - 1_000;

#[test]
fn withdrawing_half_the_position_returns_half_of_both_vaults() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let lp = pool.lp();
    let (lp_x, lp_y) = (
        pool.ata(&lp.pubkey(), &pool.mint_x),
        pool.ata(&lp.pubkey(), &pool.mint_y),
    );
    let (before_x, before_y) = (pool.balance(&lp_x), pool.balance(&lp_y));

    let burned = LP_TOKENS / 2;
    pool.withdraw(&lp, burned, 0, 0).unwrap();

    assert_eq!(pool.balance(&lp_x) - before_x, 49_999_999_750);
    assert_eq!(pool.balance(&lp_y) - before_y, 199_999_999_000);
    assert_eq!(pool.lp_supply(), 200_000_000_000 - burned);
    assert_eq!(
        pool.balance(&pool.ata(&lp.pubkey(), &pool.lp_mint)),
        LP_TOKENS - burned
    );
}

#[test]
fn the_locked_liquidity_can_never_be_withdrawn() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let lp = pool.lp();

    pool.withdraw(&lp, LP_TOKENS, 0, 0).unwrap();

    // The whole position is out, and what stays behind is the locked share.
    assert_eq!(pool.balance(&pool.ata(&lp.pubkey(), &pool.lp_mint)), 0);
    assert_eq!(pool.lp_supply(), amm::constants::MINIMUM_LIQUIDITY);
    assert!(pool.balance(&pool.vault_x) > 0);
    assert!(pool.balance(&pool.vault_y) > 0);
}

#[test]
fn liquidity_providers_collect_the_trading_fees() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let lp = pool.lp();
    let trader = pool.trader();
    let lp_x = pool.ata(&lp.pubkey(), &pool.mint_x);
    let before_x = pool.balance(&lp_x);

    // A round trip through the pool: the trader ends up with less than it
    // started with and the difference is left behind for the providers.
    pool.swap(&trader, pool.mint_x, pool.mint_y, 1_000_000_000, 0)
        .unwrap();
    pool.swap(&trader, pool.mint_y, pool.mint_x, 3_948_632_137, 0)
        .unwrap();

    pool.withdraw(&lp, LP_TOKENS, 0, 0).unwrap();

    assert!(
        pool.balance(&lp_x) - before_x > DEPOSIT_X,
        "the provider should get back more x than it put in"
    );
}

#[test]
fn rejects_a_withdrawal_below_the_caller_minimums() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let lp = pool.lp();

    assert_amm_error(
        pool.withdraw(&lp, LP_TOKENS / 2, 49_999_999_751, 0),
        AmmError::SlippageExceeded,
    );
    assert_amm_error(
        pool.withdraw(&lp, LP_TOKENS / 2, 0, 199_999_999_001),
        AmmError::SlippageExceeded,
    );
}

#[test]
fn rejects_a_withdrawal_of_nothing() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let lp = pool.lp();

    assert_amm_error(pool.withdraw(&lp, 0, 0, 0), AmmError::ZeroAmount);
}

#[test]
fn rejects_burning_more_lp_tokens_than_the_caller_holds() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let lp = pool.lp();

    // The curve allows it, the burn does not.
    assert!(pool.withdraw(&lp, LP_TOKENS + 500, 0, 0).is_err());
}

#[test]
fn a_locked_pool_still_lets_its_providers_out() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let admin = pool.admin();
    let lp = pool.lp();
    let lp_x = pool.ata(&lp.pubkey(), &pool.mint_x);
    let before_x = pool.balance(&lp_x);

    pool.set_locked(&admin, true).unwrap();

    // Trading is stopped, but a proportional exit cannot move the price and
    // the authority has no way to keep anyone's liquidity.
    assert_amm_error(
        pool.swap(&lp, pool.mint_x, pool.mint_y, 1_000_000, 0),
        AmmError::PoolLocked,
    );
    pool.withdraw(&lp, LP_TOKENS / 2, 0, 0).unwrap();

    assert_eq!(pool.balance(&lp_x) - before_x, 49_999_999_750);
}
