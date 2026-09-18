use anchor_lang::prelude::*;

use crate::{constants::*, error::AmmError, events::PoolLockSet, state::Config};

/// Stops and restarts a pool. Deposits and swaps refuse to run while it is
/// locked. Withdrawals do not: see the note in `withdraw`.
#[derive(Accounts)]
pub struct SetLocked<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [CONFIG_SEED, &config.seed.to_le_bytes()],
        bump = config.bump,
        has_one = authority @ AmmError::Unauthorized,
    )]
    pub config: Account<'info, Config>,
}

impl SetLocked<'_> {
    pub fn handler(&mut self, locked: bool) -> Result<()> {
        self.config.locked = locked;

        emit!(PoolLockSet {
            config: self.config.key(),
            locked,
        });

        Ok(())
    }
}
