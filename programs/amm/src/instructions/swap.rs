use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};

use crate::{constants::*, curve, error::AmmError, events::Swapped, state::Config};

/// Swaps one pool mint for the other.
///
/// Direction comes from the mints the caller passes, checked against the
/// pair in either order, rather than from a flag. Half the accounts of the
/// flag version, and no branch in the handler.
#[derive(Accounts)]
pub struct Swap<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    pub mint_in: Box<InterfaceAccount<'info, Mint>>,
    pub mint_out: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        seeds = [CONFIG_SEED, &config.seed.to_le_bytes()],
        bump = config.bump,
        constraint = config.holds_pair(&mint_in.key(), &mint_out.key()) @ AmmError::InvalidMintPair,
    )]
    pub config: Box<Account<'info, Config>>,

    #[account(
        mut,
        associated_token::mint = mint_in,
        associated_token::authority = config,
        associated_token::token_program = token_program,
    )]
    pub vault_in: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_out,
        associated_token::authority = config,
        associated_token::token_program = token_program,
    )]
    pub vault_out: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [TREASURY_SEED, config.key().as_ref(), mint_in.key().as_ref()],
        bump = config.treasury_bump(&mint_in.key()),
    )]
    pub treasury_in: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_in,
        associated_token::authority = user,
        associated_token::token_program = token_program,
    )]
    pub user_in: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint_out,
        associated_token::authority = user,
        associated_token::token_program = token_program,
    )]
    pub user_out: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> Swap<'info> {
    pub fn handler(&mut self, amount_in: u64, min_amount_out: u64) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);

        let amounts = curve::compute_swap(
            self.vault_in.amount,
            self.vault_out.amount,
            amount_in,
            self.config.fee_bps,
            self.config.protocol_fee_bps,
        )?;
        require!(
            amounts.amount_out >= min_amount_out,
            AmmError::SlippageExceeded
        );

        // Trader pays the protocol cut directly to the treasury: the vault
        // then holds exactly what backs the invariant, with no accrued-fee
        // field to drift out of step with the balance. The LP share stays.
        if amounts.protocol_fee > 0 {
            self.pull_in(&self.treasury_in, amounts.protocol_fee)?;
        }
        self.pull_in(&self.vault_in, amount_in - amounts.protocol_fee)?;
        self.pay_out(amounts.amount_out)?;

        emit!(Swapped {
            config: self.config.key(),
            user: self.user.key(),
            mint_in: self.mint_in.key(),
            amount_in,
            amount_out: amounts.amount_out,
            lp_fee: amounts.lp_fee,
            protocol_fee: amounts.protocol_fee,
        });

        Ok(())
    }

    fn pull_in(&self, to: &InterfaceAccount<'info, TokenAccount>, amount: u64) -> Result<()> {
        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from: self.user_in.to_account_info(),
                    mint: self.mint_in.to_account_info(),
                    to: to.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            amount,
            self.mint_in.decimals,
        )
    }

    fn pay_out(&self, amount: u64) -> Result<()> {
        let seed = self.config.seed.to_le_bytes();
        let bump = [self.config.bump];
        let signer_seeds: &[&[&[u8]]] = &[&[CONFIG_SEED, &seed, &bump]];

        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from: self.vault_out.to_account_info(),
                    mint: self.mint_out.to_account_info(),
                    to: self.user_out.to_account_info(),
                    authority: self.config.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
            self.mint_out.decimals,
        )
    }
}
