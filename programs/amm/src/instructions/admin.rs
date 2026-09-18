use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};

use crate::{
    constants::*,
    error::AmmError,
    events::{FeesWithdrawn, PoolLockSet},
    state::Config,
};

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

/// Moves collected protocol fees out of one of the two treasuries.
#[derive(Accounts)]
pub struct WithdrawFees<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        seeds = [CONFIG_SEED, &config.seed.to_le_bytes()],
        bump = config.bump,
        has_one = authority @ AmmError::Unauthorized,
    )]
    pub config: Box<Account<'info, Config>>,

    #[account(
        mut,
        seeds = [TREASURY_SEED, config.key().as_ref(), mint.key().as_ref()],
        bump = config.treasury_bump(&mint.key()),
    )]
    pub treasury: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = authority,
        associated_token::mint = mint,
        associated_token::authority = authority,
        associated_token::token_program = token_program,
    )]
    pub destination: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl WithdrawFees<'_> {
    pub fn handler(&mut self, amount: u64) -> Result<()> {
        require!(amount > 0, AmmError::ZeroAmount);
        require!(
            amount <= self.treasury.amount,
            AmmError::InsufficientTreasuryBalance
        );

        let seed = self.config.seed.to_le_bytes();
        let bump = [self.config.bump];
        let signer_seeds: &[&[&[u8]]] = &[&[CONFIG_SEED, &seed, &bump]];

        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from: self.treasury.to_account_info(),
                    mint: self.mint.to_account_info(),
                    to: self.destination.to_account_info(),
                    authority: self.config.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
            self.mint.decimals,
        )?;

        emit!(FeesWithdrawn {
            config: self.config.key(),
            mint: self.mint.key(),
            amount,
        });

        Ok(())
    }
}
