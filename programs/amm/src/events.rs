use anchor_lang::prelude::*;

#[event]
pub struct PoolCreated {
    pub config: Pubkey,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub fee_bps: u16,
    pub protocol_fee_bps: u16,
}

#[event]
pub struct LiquidityDeposited {
    pub config: Pubkey,
    pub user: Pubkey,
    pub amount_x: u64,
    pub amount_y: u64,
    pub lp_tokens: u64,
}

#[event]
pub struct LiquidityWithdrawn {
    pub config: Pubkey,
    pub user: Pubkey,
    pub amount_x: u64,
    pub amount_y: u64,
    pub lp_tokens: u64,
}

#[event]
pub struct Swapped {
    pub config: Pubkey,
    pub user: Pubkey,
    pub mint_in: Pubkey,
    pub amount_in: u64,
    pub amount_out: u64,
    pub lp_fee: u64,
}
