use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        burn, transfer_checked, Burn, Mint, TokenAccount, TokenInterface, TransferChecked,
    },
};

use crate::{constants::*, curve, error::AmmError, events::LiquidityWithdrawn, state::Config};

/// Burns LP tokens and pays out the matching share of both vaults.
#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(address = config.mint_x)]
    pub mint_x: Box<InterfaceAccount<'info, Mint>>,

    #[account(address = config.mint_y)]
    pub mint_y: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        seeds = [CONFIG_SEED, &config.seed.to_le_bytes()],
        bump = config.bump,
    )]
    pub config: Box<Account<'info, Config>>,

    #[account(
        mut,
        seeds = [LP_MINT_SEED, config.key().as_ref()],
        bump = config.lp_bump,
    )]
    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = config,
        associated_token::token_program = token_program,
    )]
    pub vault_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = config,
        associated_token::token_program = token_program,
    )]
    pub vault_y: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = user,
        associated_token::token_program = token_program,
    )]
    pub user_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = user,
        associated_token::token_program = token_program,
    )]
    pub user_y: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = lp_mint,
        associated_token::authority = user,
        associated_token::token_program = token_program,
    )]
    pub user_lp: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> Withdraw<'info> {
    /// `min_x` and `min_y` are the slippage floor for burning `lp_tokens`.
    pub fn handler(&mut self, lp_tokens: u64, min_x: u64, min_y: u64) -> Result<()> {
        // Deliberately not gated on `config.locked`. A proportional exit
        // cannot move the price or take more than its share, so there is
        // nothing for a lock to protect here, and gating it would let the
        // authority strand liquidity.
        let (amount_x, amount_y) = curve::compute_withdraw(
            self.vault_x.amount,
            self.vault_y.amount,
            self.lp_mint.supply,
            lp_tokens,
        )?;
        require!(
            amount_x >= min_x && amount_y >= min_y,
            AmmError::SlippageExceeded
        );

        // Effects before interactions. A failed CPI would revert the whole
        // instruction anyway, but the ordering holds if that ever changes.
        self.burn_lp(lp_tokens)?;
        self.pay_out(&self.mint_x, &self.vault_x, &self.user_x, amount_x)?;
        self.pay_out(&self.mint_y, &self.vault_y, &self.user_y, amount_y)?;

        emit!(LiquidityWithdrawn {
            config: self.config.key(),
            user: self.user.key(),
            amount_x,
            amount_y,
            lp_tokens,
        });

        Ok(())
    }

    fn burn_lp(&self, amount: u64) -> Result<()> {
        burn(
            CpiContext::new(
                self.token_program.key(),
                Burn {
                    mint: self.lp_mint.to_account_info(),
                    from: self.user_lp.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            amount,
        )
    }

    fn pay_out(
        &self,
        mint: &InterfaceAccount<'info, Mint>,
        from: &InterfaceAccount<'info, TokenAccount>,
        to: &InterfaceAccount<'info, TokenAccount>,
        amount: u64,
    ) -> Result<()> {
        let seed = self.config.seed.to_le_bytes();
        let bump = [self.config.bump];
        let signer_seeds: &[&[&[u8]]] = &[&[CONFIG_SEED, &seed, &bump]];

        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from: from.to_account_info(),
                    mint: mint.to_account_info(),
                    to: to.to_account_info(),
                    authority: self.config.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
            mint.decimals,
        )
    }
}
