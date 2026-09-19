mod common;

use {amm::error::AmmError, common::*};

#[test]
fn creates_a_pool_ready_to_receive_liquidity() {
    let mut pool = Pool::new();
    pool.initialize(FEE_BPS, PROTOCOL_FEE_BPS).unwrap();

    let config = pool.config_state();
    assert_eq!(config.seed, POOL_SEED);
    assert_eq!(config.authority, pool.admin.pubkey());
    assert_eq!(config.mint_x, pool.mint_x);
    assert_eq!(config.mint_y, pool.mint_y);
    assert_eq!(config.fee_bps, FEE_BPS);
    assert_eq!(config.protocol_fee_bps, PROTOCOL_FEE_BPS);
    assert!(!config.locked);

    assert_eq!(pool.balance(&pool.vault_x), 0);
    assert_eq!(pool.balance(&pool.vault_y), 0);
    assert_eq!(pool.balance(&pool.treasury_x), 0);
    assert_eq!(pool.balance(&pool.treasury_y), 0);
    assert_eq!(pool.balance(&pool.locked_lp), 0);
    assert_eq!(pool.lp_supply(), 0);
}

#[test]
fn stores_the_bumps_the_instructions_rely_on() {
    let mut pool = Pool::new();
    pool.initialize(FEE_BPS, PROTOCOL_FEE_BPS).unwrap();

    let config = pool.config_state();
    assert_eq!(config.treasury_bump(&pool.mint_x), config.treasury_x_bump);
    assert_eq!(config.treasury_bump(&pool.mint_y), config.treasury_y_bump);
    assert!(config.holds_pair(&pool.mint_x, &pool.mint_y));
    assert!(config.holds_pair(&pool.mint_y, &pool.mint_x));
}

#[test]
fn rejects_a_fee_above_the_maximum() {
    let mut pool = Pool::new();

    assert_amm_error(
        pool.initialize(1_001, PROTOCOL_FEE_BPS),
        AmmError::FeeTooHigh,
    );
}

#[test]
fn rejects_a_protocol_share_above_the_whole_fee() {
    let mut pool = Pool::new();

    assert_amm_error(
        pool.initialize(FEE_BPS, 10_001),
        AmmError::ProtocolFeeTooHigh,
    );
}

#[test]
fn rejects_a_pool_of_one_mint_against_itself() {
    let mut pool = Pool::new();
    let mint_x = pool.mint_x;

    assert_amm_error(
        pool.initialize_pair(mint_x, mint_x, FEE_BPS, PROTOCOL_FEE_BPS),
        AmmError::IdenticalMints,
    );
}

#[test]
fn rejects_a_pool_whose_mints_use_different_token_programs() {
    let mut pool = Pool::new();
    let mint_y = pool.create_mint_owned_by(9, TOKEN_2022_PROGRAM_ID);
    let mint_x = pool.mint_x;

    // The pool passes one token program to every CPI it makes, so a pair
    // split across SPL Token and Token-2022 is turned away up front.
    assert_amm_error(
        pool.initialize_pair(mint_x, mint_y, FEE_BPS, PROTOCOL_FEE_BPS),
        AmmError::MintProgramMismatch,
    );
}

#[test]
fn rejects_a_mint_that_carries_extensions() {
    let mut pool = Pool::new();
    let mint_x = pool.create_mint_with_transfer_fee(6);
    let mint_y = pool.create_mint_owned_by(9, TOKEN_2022_PROGRAM_ID);

    // Both mints are Token-2022, so the pair itself is fine. The transfer fee
    // on the first one is not: it would take a cut on the way into the vault
    // and leave the reserves short of what the curve priced.
    assert_amm_error(
        pool.initialize_pair_under(
            mint_x,
            mint_y,
            TOKEN_2022_PROGRAM_ID,
            FEE_BPS,
            PROTOCOL_FEE_BPS,
        ),
        AmmError::MintHasExtensions,
    );
}

#[test]
fn rejects_a_second_pool_on_the_same_seed() {
    let mut pool = Pool::new();
    pool.initialize(FEE_BPS, PROTOCOL_FEE_BPS).unwrap();

    assert!(pool.initialize(FEE_BPS, PROTOCOL_FEE_BPS).is_err());
}
