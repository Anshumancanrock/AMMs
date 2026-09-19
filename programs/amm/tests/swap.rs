mod common;

use {amm::error::AmmError, common::*};

const DEPOSIT_X: u64 = 100_000_000_000;
const DEPOSIT_Y: u64 = 400_000_000_000;

/// Swapping 1_000_000_000 x into a 1e11 / 4e11 pool at a 0.3% fee:
///
/// ```text
/// fee   = ceil(1e9 * 30 / 10_000)        = 3_000_000
/// cut   = 3_000_000 * 2_000 / 10_000     =   600_000   -> treasury
/// net   = 1e9 - fee                      = 997_000_000
/// out   = 4e11 * net / (1e11 + net)      = 3_948_632_137
/// ```
const SWAP_IN: u64 = 1_000_000_000;
const SWAP_OUT: u64 = 3_948_632_137;
const PROTOCOL_CUT: u64 = 600_000;
const LP_CUT: u64 = 2_400_000;

#[test]
fn swapping_x_for_y_pays_the_curve_price() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();
    let trader_y = pool.ata(&trader.pubkey(), &pool.mint_y);
    let before_y = pool.balance(&trader_y);

    pool.swap(&trader, pool.mint_x, pool.mint_y, SWAP_IN, SWAP_OUT)
        .unwrap();

    assert_eq!(pool.balance(&trader_y) - before_y, SWAP_OUT);
    assert_eq!(pool.balance(&pool.vault_y), DEPOSIT_Y - SWAP_OUT);
    // The vault keeps the input minus the protocol cut, which went straight to
    // the treasury.
    assert_eq!(
        pool.balance(&pool.vault_x),
        DEPOSIT_X + SWAP_IN - PROTOCOL_CUT
    );
    assert_eq!(pool.balance(&pool.treasury_x), PROTOCOL_CUT);
    assert_eq!(pool.balance(&pool.treasury_y), 0);
}

#[test]
fn swapping_y_for_x_works_in_the_other_direction() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();
    let trader_x = pool.ata(&trader.pubkey(), &pool.mint_x);
    let before_x = pool.balance(&trader_x);

    // 4e9 y into the same pool: fee 12_000_000, cut 2_400_000, out 987_158_034.
    pool.swap(&trader, pool.mint_y, pool.mint_x, 4_000_000_000, 0)
        .unwrap();

    assert_eq!(pool.balance(&trader_x) - before_x, 987_158_034);
    assert_eq!(pool.balance(&pool.treasury_y), 2_400_000);
    assert_eq!(pool.balance(&pool.treasury_x), 0);
}

#[test]
fn every_swap_leaves_the_invariant_at_least_where_it_was() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();
    let (mint_x, mint_y) = (pool.mint_x, pool.mint_y);
    let mut invariant = pool.invariant();

    for amount_in in [1_000_000u64, 250_000_000, 7_777_777_777] {
        pool.swap(&trader, mint_x, mint_y, amount_in, 0).unwrap();
        pool.swap(&trader, mint_y, mint_x, amount_in * 4, 0)
            .unwrap();

        let next = pool.invariant();
        assert!(
            next > invariant,
            "invariant dropped from {invariant} to {next}"
        );
        invariant = next;
    }
}

#[test]
fn the_lp_share_of_the_fee_stays_in_the_pool() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();

    pool.swap(&trader, pool.mint_x, pool.mint_y, SWAP_IN, 0)
        .unwrap();

    // Of the 3_000_000 charged on the way in, the treasury took 600_000 and
    // the other 2_400_000 stayed in the vault on top of the amount the curve
    // was actually priced against.
    let priced_against = SWAP_IN - LP_CUT - PROTOCOL_CUT;
    assert_eq!(pool.balance(&pool.treasury_x), PROTOCOL_CUT);
    assert_eq!(
        pool.balance(&pool.vault_x),
        DEPOSIT_X + priced_against + LP_CUT
    );
    assert!(pool.invariant() > (DEPOSIT_X as u128) * (DEPOSIT_Y as u128));
}

#[test]
fn rejects_a_swap_that_pays_less_than_the_caller_accepts() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();

    assert_amm_error(
        pool.swap(&trader, pool.mint_x, pool.mint_y, SWAP_IN, SWAP_OUT + 1),
        AmmError::SlippageExceeded,
    );
}

#[test]
fn rejects_a_swap_of_nothing() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();

    assert_amm_error(
        pool.swap(&trader, pool.mint_x, pool.mint_y, 0, 0),
        AmmError::ZeroAmount,
    );
}

#[test]
fn rejects_a_swap_against_an_empty_pool() {
    let mut pool = Pool::opened();
    let trader = pool.trader();

    assert_amm_error(
        pool.swap(&trader, pool.mint_x, pool.mint_y, 1_000_000, 0),
        AmmError::PoolEmpty,
    );
}

#[test]
fn rejects_a_mint_that_does_not_belong_to_the_pool() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();
    let foreign = pool.create_mint(6);
    pool.create_ata(&trader.pubkey(), &foreign);

    // The pool never opened a vault for this mint, so there is no account at
    // the address the instruction derives for it.
    assert_custom_error(
        pool.swap(&trader, foreign, pool.mint_y, 1_000_000, 0),
        u32::from(anchor_lang::error::ErrorCode::AccountNotInitialized),
    );
}

#[test]
fn rejects_a_swap_of_a_mint_for_itself() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();

    // Both sides resolve to the same vault, which Anchor refuses before the
    // handler ever runs.
    assert_custom_error(
        pool.swap(&trader, pool.mint_x, pool.mint_x, 1_000_000, 0),
        u32::from(anchor_lang::error::ErrorCode::ConstraintDuplicateMutableAccount),
    );
}

#[test]
fn rejects_a_swap_that_targets_the_lp_mint() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();

    // Every account in this one exists: the pool holds an associated token
    // account for its own LP mint, the one with the locked liquidity in it.
    // Only the pair check stands between a trader and those tokens.
    assert_amm_error(
        pool.swap(&trader, pool.mint_x, pool.lp_mint, 1_000_000, 0),
        AmmError::InvalidMintPair,
    );
}

#[test]
fn rejects_a_swap_while_the_pool_is_locked() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let admin = pool.admin();
    let trader = pool.trader();
    pool.set_locked(&admin, true).unwrap();

    assert_amm_error(
        pool.swap(&trader, pool.mint_x, pool.mint_y, SWAP_IN, 0),
        AmmError::PoolLocked,
    );
}
