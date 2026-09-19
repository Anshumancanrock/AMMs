mod common;

use {amm::error::AmmError, common::*, solana_keypair::Keypair, solana_signer::Signer};

const DEPOSIT_X: u64 = 100_000_000_000;
const DEPOSIT_Y: u64 = 400_000_000_000;

fn pool_with_fees_collected() -> Pool {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let trader = pool.trader();
    pool.swap(&trader, pool.mint_x, pool.mint_y, 1_000_000_000, 0)
        .unwrap();
    pool
}

#[test]
fn the_authority_can_lock_and_unlock_the_pool() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let admin = pool.admin();
    let trader = pool.trader();

    pool.set_locked(&admin, true).unwrap();
    assert!(pool.config_state().locked);
    assert_amm_error(
        pool.swap(&trader, pool.mint_x, pool.mint_y, 1_000_000, 0),
        AmmError::PoolLocked,
    );

    pool.set_locked(&admin, false).unwrap();
    assert!(!pool.config_state().locked);
    pool.swap(&trader, pool.mint_x, pool.mint_y, 1_000_000, 0)
        .unwrap();
}

#[test]
fn nobody_else_can_lock_the_pool() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let stranger = Keypair::new();
    pool.svm.airdrop(&stranger.pubkey(), 1_000_000_000).unwrap();

    assert_amm_error(pool.set_locked(&stranger, true), AmmError::Unauthorized);
    assert!(!pool.config_state().locked);
}

#[test]
fn the_authority_can_move_the_collected_fees_out() {
    let mut pool = pool_with_fees_collected();
    let admin = pool.admin();
    let collected = pool.balance(&pool.treasury_x);
    assert_eq!(collected, 600_000);

    pool.withdraw_fees(&admin, pool.mint_x, collected).unwrap();

    assert_eq!(pool.balance(&pool.treasury_x), 0);
    assert_eq!(
        pool.balance(&pool.ata(&admin.pubkey(), &pool.mint_x)),
        collected
    );
}

#[test]
fn fees_can_be_taken_out_in_parts() {
    let mut pool = pool_with_fees_collected();
    let admin = pool.admin();

    pool.withdraw_fees(&admin, pool.mint_x, 100_000).unwrap();
    pool.withdraw_fees(&admin, pool.mint_x, 250_000).unwrap();

    assert_eq!(pool.balance(&pool.treasury_x), 250_000);
    assert_eq!(
        pool.balance(&pool.ata(&admin.pubkey(), &pool.mint_x)),
        350_000
    );
}

#[test]
fn rejects_a_fee_withdrawal_larger_than_the_treasury() {
    let mut pool = pool_with_fees_collected();
    let admin = pool.admin();

    assert_amm_error(
        pool.withdraw_fees(&admin, pool.mint_x, 600_001),
        AmmError::InsufficientTreasuryBalance,
    );
}

#[test]
fn rejects_a_fee_withdrawal_of_nothing() {
    let mut pool = pool_with_fees_collected();
    let admin = pool.admin();

    assert_amm_error(
        pool.withdraw_fees(&admin, pool.mint_x, 0),
        AmmError::ZeroAmount,
    );
}

#[test]
fn nobody_else_can_take_the_fees() {
    let mut pool = pool_with_fees_collected();
    let stranger = Keypair::new();
    pool.svm.airdrop(&stranger.pubkey(), 1_000_000_000).unwrap();

    assert_amm_error(
        pool.withdraw_fees(&stranger, pool.mint_x, 600_000),
        AmmError::Unauthorized,
    );
    assert_eq!(pool.balance(&pool.treasury_x), 600_000);
}

#[test]
fn rejects_a_fee_withdrawal_for_a_mint_outside_the_pool() {
    let mut pool = pool_with_fees_collected();
    let admin = pool.admin();
    let foreign = pool.create_mint(6);

    // The treasury seeds carry the mint, so a mint this pool never opened
    // derives an account that does not exist.
    assert_custom_error(
        pool.withdraw_fees(&admin, foreign, 1),
        u32::from(anchor_lang::error::ErrorCode::AccountNotInitialized),
    );
}

#[test]
fn the_authority_can_never_touch_the_vaults() {
    let mut pool = pool_with_fees_collected();
    let admin = pool.admin();
    let vault_balance = pool.balance(&pool.vault_x);

    // `withdraw_fees` derives its source account from the treasury seeds, so
    // handing it a vault fails the seed check instead of draining the pool.
    let result = pool.withdraw_fees_from(&admin, pool.mint_x, pool.vault_x, vault_balance);

    assert_custom_error(
        result,
        u32::from(anchor_lang::error::ErrorCode::ConstraintSeeds),
    );
    assert_eq!(pool.balance(&pool.vault_x), vault_balance);
}
