use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use crate::{constants::*, error::AmmError, state::Config};

/// Creates the config, the LP mint, both vaults, both fee treasuries and the
/// account that holds the locked liquidity.
///
/// Both mints must sit under the same token program, which keeps one
/// `token_program` account for every later CPI. Neither may carry Token-2022
/// extensions: a transfer fee delivers less to the vault than the curve
/// priced, and a permanent delegate can move the reserves at will.
#[derive(Accounts)]
#[instruction(seed: u64)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        constraint = mint_x.key() != mint_y.key() @ AmmError::IdenticalMints,
        constraint = *mint_x.to_account_info().owner == token_program.key() @ AmmError::MintProgramMismatch,
    )]
    pub mint_x: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        constraint = *mint_y.to_account_info().owner == token_program.key() @ AmmError::MintProgramMismatch,
    )]
    pub mint_y: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = admin,
        space = 8 + Config::INIT_SPACE,
        seeds = [CONFIG_SEED, &seed.to_le_bytes()],
        bump,
    )]
    pub config: Box<Account<'info, Config>>,

    #[account(
        init,
        payer = admin,
        seeds = [LP_MINT_SEED, config.key().as_ref()],
        bump,
        mint::decimals = LP_DECIMALS,
        mint::authority = config,
        mint::token_program = token_program,
    )]
    pub lp_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = admin,
        associated_token::mint = mint_x,
        associated_token::authority = config,
        associated_token::token_program = token_program,
    )]
    pub vault_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        payer = admin,
        associated_token::mint = mint_y,
        associated_token::authority = config,
        associated_token::token_program = token_program,
    )]
    pub vault_y: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        payer = admin,
        seeds = [TREASURY_SEED, config.key().as_ref(), mint_x.key().as_ref()],
        bump,
        token::mint = mint_x,
        token::authority = config,
        token::token_program = token_program,
    )]
    pub treasury_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        payer = admin,
        seeds = [TREASURY_SEED, config.key().as_ref(), mint_y.key().as_ref()],
        bump,
        token::mint = mint_y,
        token::authority = config,
        token::token_program = token_program,
    )]
    pub treasury_y: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl Initialize<'_> {
    pub fn handler(
        &mut self,
        seed: u64,
        fee_bps: u16,
        protocol_fee_bps: u16,
        authority: Pubkey,
        bumps: &InitializeBumps,
    ) -> Result<()> {
        require!(fee_bps <= MAX_FEE_BPS, AmmError::FeeTooHigh);
        require!(
            protocol_fee_bps as u64 <= BPS_DENOMINATOR,
            AmmError::ProtocolFeeTooHigh
        );

        self.config.set_inner(Config {
            seed,
            authority,
            mint_x: self.mint_x.key(),
            mint_y: self.mint_y.key(),
            fee_bps,
            protocol_fee_bps,
            locked: false,
            bump: bumps.config,
            lp_bump: bumps.lp_mint,
            treasury_x_bump: bumps.treasury_x,
            treasury_y_bump: bumps.treasury_y,
        });

        Ok(())
    }
}
