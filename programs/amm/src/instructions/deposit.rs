use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        mint_to, transfer_checked, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
    },
};

use crate::{constants::*, curve, error::AmmError, events::LiquidityDeposited, state::Config};

/// Adds liquidity at the current pool ratio and mints LP tokens for it.
#[derive(Accounts)]
pub struct Deposit<'info> {
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
        init_if_needed,
        payer = user,
        associated_token::mint = lp_mint,
        associated_token::authority = user,
        associated_token::token_program = token_program,
    )]
    pub user_lp: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> Deposit<'info> {
    /// `max_x` and `max_y` are ceilings: the pool takes what the ratio
    /// allows and leaves the rest. `min_lp` is the slippage floor.
    pub fn handler(&mut self, max_x: u64, max_y: u64, min_lp: u64) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);

        let amounts = curve::compute_deposit(
            self.vault_x.amount,
            self.vault_y.amount,
            self.lp_mint.supply,
            max_x,
            max_y,
        )?;
        require!(amounts.lp_tokens >= min_lp, AmmError::SlippageExceeded);

        self.pull_in(&self.mint_x, &self.user_x, &self.vault_x, amounts.amount_x)?;
        self.pull_in(&self.mint_y, &self.user_y, &self.vault_y, amounts.amount_y)?;

        self.mint_lp(&self.user_lp, amounts.lp_tokens)?;

        emit!(LiquidityDeposited {
            config: self.config.key(),
            user: self.user.key(),
            amount_x: amounts.amount_x,
            amount_y: amounts.amount_y,
            lp_tokens: amounts.lp_tokens,
        });

        Ok(())
    }

    fn pull_in(
        &self,
        mint: &InterfaceAccount<'info, Mint>,
        from: &InterfaceAccount<'info, TokenAccount>,
        to: &InterfaceAccount<'info, TokenAccount>,
        amount: u64,
    ) -> Result<()> {
        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from: from.to_account_info(),
                    mint: mint.to_account_info(),
                    to: to.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            amount,
            mint.decimals,
        )
    }

    fn mint_lp(&self, to: &InterfaceAccount<'info, TokenAccount>, amount: u64) -> Result<()> {
        let seed = self.config.seed.to_le_bytes();
        let bump = [self.config.bump];
        let signer_seeds: &[&[&[u8]]] = &[&[CONFIG_SEED, &seed, &bump]];

        mint_to(
            CpiContext::new_with_signer(
                self.token_program.key(),
                MintTo {
                    mint: self.lp_mint.to_account_info(),
                    to: to.to_account_info(),
                    authority: self.config.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
        )
    }
}
