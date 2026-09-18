use anchor_lang::prelude::*;

#[event]
pub struct PoolCreated {
    pub config: Pubkey,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub fee_bps: u16,
    pub protocol_fee_bps: u16,
}
