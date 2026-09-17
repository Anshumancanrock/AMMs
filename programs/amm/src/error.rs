use anchor_lang::prelude::*;

#[error_code]
pub enum AmmError {
    #[msg("Swap fee is above the 10% maximum")]
    FeeTooHigh,
    #[msg("Protocol fee cannot take more than the whole swap fee")]
    ProtocolFeeTooHigh,
    #[msg("A pool needs two different mints")]
    IdenticalMints,
    #[msg("Both mints must belong to the token program passed in")]
    MintProgramMismatch,
    #[msg("Pool is locked")]
    PoolLocked,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Pool holds no liquidity")]
    PoolEmpty,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("First deposit must mint more than the minimum liquidity")]
    InsufficientInitialLiquidity,
    #[msg("Result is worse than the limit the caller asked for")]
    SlippageExceeded,
    #[msg("Mints do not belong to this pool")]
    InvalidMintPair,
    #[msg("Only the pool authority can call this")]
    Unauthorized,
}
