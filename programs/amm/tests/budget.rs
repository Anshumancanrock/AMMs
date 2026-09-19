//! Compute ceilings, so a change that doubles the cost of a swap fails here
//! first. `cargo test --test budget -- --nocapture` prints the numbers.
//!
//! The ceilings sit well clear of the measured cost: the mints are random per
//! run, and hunting the canonical bump for their PDAs and associated token
//! addresses moves the total by tens of thousands of units.

mod common;

use common::*;

const DEPOSIT_X: u64 = 100_000_000_000;
const DEPOSIT_Y: u64 = 400_000_000_000;

fn report(label: &str, used: u64, ceiling: u64) {
    println!("{label:<16} {used:>6} CU (ceiling {ceiling})");
    assert!(
        used <= ceiling,
        "{label} used {used} CU, over the {ceiling} ceiling"
    );
}

#[test]
fn every_instruction_stays_inside_its_budget() {
    let mut pool = Pool::new();
    let admin = pool.admin();
    let lp = pool.lp();
    let trader = pool.trader();

    let used = pool.initialize(FEE_BPS, PROTOCOL_FEE_BPS).unwrap();
    report("initialize", used.compute_units_consumed, 180_000);

    // The first deposit also creates the depositor's LP token account.
    let used = pool.deposit(&lp, DEPOSIT_X, DEPOSIT_Y, 0).unwrap();
    report("first deposit", used.compute_units_consumed, 140_000);

    let used = pool.deposit(&lp, 1_000_000, 4_000_000, 0).unwrap();
    report("deposit", used.compute_units_consumed, 80_000);

    let used = pool
        .swap(&trader, pool.mint_x, pool.mint_y, 1_000_000_000, 0)
        .unwrap();
    report("swap", used.compute_units_consumed, 75_000);

    let used = pool.withdraw(&lp, 1_000_000, 0, 0).unwrap();
    report("withdraw", used.compute_units_consumed, 80_000);

    let used = pool.set_locked(&admin, true).unwrap();
    report("set_locked", used.compute_units_consumed, 8_000);

    let used = pool.withdraw_fees(&admin, pool.mint_x, 1).unwrap();
    report("withdraw_fees", used.compute_units_consumed, 75_000);
}
