//! Account substitution and economic attacks, written from the attacker's
//! side rather than as assertions about the checks that stop them.

mod common;

use {anchor_lang::prelude::Pubkey, common::*, solana_keypair::Keypair};

const DEPOSIT_X: u64 = 100_000_000_000;
const DEPOSIT_Y: u64 = 400_000_000_000;

#[test]
fn a_vault_cannot_be_swapped_for_an_account_the_attacker_owns() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let attacker = pool.trader();
    let (mint_x, mint_y, vault_in) = (pool.mint_x, pool.mint_y, pool.vault_x);

    // Point the payout vault at the attacker's own account for that mint.
    let own_account = pool.ata(&attacker.pubkey(), &mint_y);
    let result = pool.swap_from(
        &attacker,
        mint_x,
        mint_y,
        vault_in,
        own_account,
        1_000_000_000,
        0,
    );

    assert!(result.is_err());
    assert_eq!(pool.balance(&pool.vault_y), DEPOSIT_Y);
}

#[test]
fn one_pool_cannot_reach_another_pools_vaults() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let attacker = pool.trader();
    let (mint_x, mint_y, vault_in) = (pool.mint_x, pool.mint_y, pool.vault_x);

    // A second pool on the same mints, funded, so worth draining.
    let (_, _, sibling_vault_y) = pool.open_sibling_pool(POOL_SEED + 1);
    pool.donate(&attacker, &mint_y, &sibling_vault_y, 10_000_000_000);

    // Vault addresses derive from the config that owns them.
    let result = pool.swap_from(
        &attacker,
        mint_x,
        mint_y,
        vault_in,
        sibling_vault_y,
        1_000_000_000,
        0,
    );

    assert!(result.is_err());
    assert_eq!(pool.balance(&sibling_vault_y), 10_000_000_000);
}

#[test]
fn the_treasury_cannot_be_drained_through_a_swap() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let attacker = pool.trader();
    let (mint_x, mint_y, vault_in) = (pool.mint_x, pool.mint_y, pool.vault_x);
    pool.swap(&attacker, mint_x, mint_y, 1_000_000_000, 0)
        .unwrap();
    let collected = pool.balance(&pool.treasury_x);

    let treasury_x = pool.treasury_x;
    let result = pool.swap_from(
        &attacker, mint_y, mint_x, vault_in, treasury_x, 1_000_000, 0,
    );

    assert!(result.is_err());
    assert_eq!(pool.balance(&pool.treasury_x), collected);
}

#[test]
fn inflating_the_share_price_costs_the_attacker_more_than_it_returns() {
    let mut pool = Pool::opened();
    let attacker = pool.trader();
    let victim = pool.lp();
    let attacker_x = pool.ata(&attacker.pubkey(), &pool.mint_x);
    let attacker_y = pool.ata(&attacker.pubkey(), &pool.mint_y);
    let (before_x, before_y) = (pool.balance(&attacker_x), pool.balance(&attacker_y));

    // Classic share price inflation: take the smallest first deposit the
    // pool allows, donate to move the price per LP token far above what that
    // deposit cost, then hope the next depositor's LP amount rounds down
    // into the attacker's share.
    //
    // The smallest first deposit leaves 1_000 LP locked and the attacker 1.
    pool.deposit(&attacker, 1_001, 1_001, 0).unwrap();
    assert_eq!(
        pool.balance(&pool.ata(&attacker.pubkey(), &pool.lp_mint)),
        1
    );

    let (mint_x, mint_y, vault_x, vault_y) = (pool.mint_x, pool.mint_y, pool.vault_x, pool.vault_y);
    pool.donate(&attacker, &mint_x, &vault_x, 1_000_000_000);
    pool.donate(&attacker, &mint_y, &vault_y, 1_000_000_000);

    pool.deposit(&victim, 1_000_000_000, 1_000_000_000, 0)
        .unwrap();

    let held = pool.balance(&pool.ata(&attacker.pubkey(), &pool.lp_mint));
    pool.withdraw(&attacker, held, 0, 0).unwrap();

    let (after_x, after_y) = (pool.balance(&attacker_x), pool.balance(&attacker_y));
    assert!(
        after_x < before_x && after_y < before_y,
        "the attack should never pay for itself"
    );
    // One LP token out of 1_001 means the locked share swallowed the
    // donation: the attacker recovers roughly a thousandth of it.
    assert!(before_x - after_x > 900_000_000);
}

#[test]
fn a_program_that_is_not_a_token_program_is_rejected() {
    let mut pool = Pool::new();
    let (mint_x, mint_y) = (pool.mint_x, pool.mint_y);

    let result = pool.initialize_pair_under(
        mint_x,
        mint_y,
        Pubkey::new_unique(),
        FEE_BPS,
        PROTOCOL_FEE_BPS,
    );

    assert!(result.is_err());
}

#[test]
fn a_stranger_cannot_move_tokens_out_of_a_vault() {
    let mut pool = Pool::funded(DEPOSIT_X, DEPOSIT_Y);
    let stranger = Keypair::new();
    pool.svm.airdrop(&stranger.pubkey(), 1_000_000_000).unwrap();
    let (mint_x, vault_x) = (pool.mint_x, pool.vault_x);
    let destination = pool.create_ata(&stranger.pubkey(), &mint_x);

    // Only this program can sign for the config PDA that owns the vault.
    let result = pool.raw_transfer(&stranger, &mint_x, &vault_x, &destination, 1_000);

    assert!(result.is_err());
    assert_eq!(pool.balance(&vault_x), DEPOSIT_X);
}
