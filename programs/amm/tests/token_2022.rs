//! The pool reaches the token programs through `token_interface`, so the
//! same code has to work with Token-2022 mints. Extensions are refused at
//! `initialize`; plain Token-2022 mints run the whole lifecycle.

mod common;

use common::*;

const DEPOSIT_X: u64 = 100_000_000_000;
const DEPOSIT_Y: u64 = 400_000_000_000;

#[test]
fn a_token_2022_pool_runs_the_whole_lifecycle() {
    let mut pool = Pool::funded_under(TOKEN_2022_PROGRAM_ID, DEPOSIT_X, DEPOSIT_Y);
    let admin = pool.admin();
    let lp = pool.lp();
    let trader = pool.trader();

    assert_eq!(pool.balance(&pool.vault_x), DEPOSIT_X);
    assert_eq!(pool.balance(&pool.vault_y), DEPOSIT_Y);
    assert_eq!(pool.lp_supply(), 200_000_000_000);

    // Same pool, same numbers as the SPL Token suite.
    pool.swap(&trader, pool.mint_x, pool.mint_y, 1_000_000_000, 0)
        .unwrap();
    assert_eq!(pool.balance(&pool.treasury_x), 600_000);
    assert_eq!(
        pool.balance(&pool.vault_x),
        DEPOSIT_X + 1_000_000_000 - 600_000
    );

    pool.withdraw_fees(&admin, pool.mint_x, 600_000).unwrap();
    assert_eq!(
        pool.balance(&pool.ata(&admin.pubkey(), &pool.mint_x)),
        600_000
    );

    let held = pool.balance(&pool.ata(&lp.pubkey(), &pool.lp_mint));
    pool.withdraw(&lp, held, 0, 0).unwrap();
    assert_eq!(pool.lp_supply(), amm::constants::MINIMUM_LIQUIDITY);
}
