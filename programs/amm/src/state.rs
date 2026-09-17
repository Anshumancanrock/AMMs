use anchor_lang::prelude::*;

/// One per pool. Owns both vaults, the LP mint and both treasuries, and is
/// the only authority the token CPIs sign with.
#[account]
#[derive(InitSpace)]
pub struct Config {
    /// Namespaces the pool, so one mint pair can have several.
    pub seed: u64,
    /// Can lock the pool and withdraw treasury fees. Nothing else.
    pub authority: Pubkey,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    /// Charged on swap input, in basis points.
    pub fee_bps: u16,
    /// The treasury's share of `fee_bps`. The remainder stays in the vault
    /// and accrues to the LPs.
    pub protocol_fee_bps: u16,
    pub locked: bool,
    pub bump: u8,
    pub lp_bump: u8,
    pub treasury_x_bump: u8,
    pub treasury_y_bump: u8,
}

impl Config {
    pub fn holds_pair(&self, mint_in: &Pubkey, mint_out: &Pubkey) -> bool {
        (mint_in == &self.mint_x && mint_out == &self.mint_y)
            || (mint_in == &self.mint_y && mint_out == &self.mint_x)
    }

    /// Treasury bump for one of the pool mints. The seeds include the mint,
    /// so a mint outside the pool derives an account that was never created.
    pub fn treasury_bump(&self, mint: &Pubkey) -> u8 {
        if mint == &self.mint_x {
            self.treasury_x_bump
        } else {
            self.treasury_y_bump
        }
    }
}
