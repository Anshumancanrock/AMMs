//! LiteSVM harness shared by the integration tests.
//!
//! Each test gets a fresh VM holding the program, two mints and three funded
//! wallets: the pool authority, a liquidity provider and a trader. `new`
//! stops short of creating the pool so `initialize` failures are reachable,
//! `opened` creates it, `funded` also deposits.

// Each binary uses a subset of the helpers, and TxResult carries LiteSVM's
// own oversized error type.
#![allow(dead_code, clippy::result_large_err)]

use {
    amm::{constants::*, error::AmmError, state::Config},
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{
            instruction::{error::InstructionError, Instruction},
            program_pack::Pack,
            system_instruction, system_program,
        },
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::{
            get_associated_token_address_with_program_id, spl_associated_token_account,
            ID as ASSOCIATED_TOKEN_PROGRAM_ID,
        },
        token::{spl_token, ID as TOKEN_PROGRAM_ID},
        token_2022::spl_token_2022::{
            extension::StateWithExtensions,
            state::{Account as TokenAccountState, Mint as MintState},
        },
    },
    litesvm::{
        types::{FailedTransactionMetadata, TransactionMetadata},
        LiteSVM,
    },
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_transaction::versioned::VersionedTransaction,
    solana_transaction_error::TransactionError,
};

pub use {anchor_spl::token_2022::ID as TOKEN_2022_PROGRAM_ID, solana_signer::Signer};

pub type TxResult = Result<TransactionMetadata, FailedTransactionMetadata>;

pub const POOL_SEED: u64 = 42;
/// 0.3% swap fee, a fifth of which is protocol revenue.
pub const FEE_BPS: u16 = 30;
pub const PROTOCOL_FEE_BPS: u16 = 2_000;
pub const DECIMALS_X: u8 = 6;
pub const DECIMALS_Y: u8 = 9;
/// Starting balance of both mints for the liquidity provider and the trader.
pub const STARTING_BALANCE: u64 = 1_000_000_000_000_000;

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

pub struct Pool {
    pub svm: LiteSVM,
    pub token_program: Pubkey,
    pub admin: Keypair,
    pub lp: Keypair,
    pub trader: Keypair,
    pub seed: u64,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub config: Pubkey,
    pub lp_mint: Pubkey,
    pub vault_x: Pubkey,
    pub vault_y: Pubkey,
    pub treasury_x: Pubkey,
    pub treasury_y: Pubkey,
    pub locked_lp: Pubkey,
}

impl Pool {
    pub fn new() -> Self {
        Self::new_under(TOKEN_PROGRAM_ID)
    }

    /// The same setup with both mints under a token program of the caller's
    /// choosing, so the suite can run the whole lifecycle on Token-2022.
    pub fn new_under(token_program: Pubkey) -> Self {
        let mut svm = LiteSVM::new();
        svm.add_program(
            amm::ID,
            include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/amm.so")),
        )
        .unwrap();

        let admin = Keypair::new();
        let lp = Keypair::new();
        let trader = Keypair::new();
        for wallet in [&admin, &lp, &trader] {
            svm.airdrop(&wallet.pubkey(), 100 * LAMPORTS_PER_SOL)
                .unwrap();
        }

        let seed = POOL_SEED;
        let config = Pubkey::find_program_address(&[CONFIG_SEED, &seed.to_le_bytes()], &amm::ID).0;
        let lp_mint = Pubkey::find_program_address(&[LP_MINT_SEED, config.as_ref()], &amm::ID).0;

        let mut pool = Self {
            token_program,
            mint_x: Pubkey::default(),
            mint_y: Pubkey::default(),
            vault_x: Pubkey::default(),
            vault_y: Pubkey::default(),
            treasury_x: Pubkey::default(),
            treasury_y: Pubkey::default(),
            locked_lp: ata_under(&config, &lp_mint, &token_program),
            svm,
            admin,
            lp,
            trader,
            seed,
            config,
            lp_mint,
        };

        pool.mint_x = pool.create_mint_owned_by(DECIMALS_X, token_program);
        pool.mint_y = pool.create_mint_owned_by(DECIMALS_Y, token_program);
        pool.vault_x = pool.ata(&pool.config, &pool.mint_x);
        pool.vault_y = pool.ata(&pool.config, &pool.mint_y);
        pool.treasury_x = treasury(&pool.config, &pool.mint_x);
        pool.treasury_y = treasury(&pool.config, &pool.mint_y);

        for wallet in [pool.lp.pubkey(), pool.trader.pubkey()] {
            for mint in [pool.mint_x, pool.mint_y] {
                let account = pool.create_ata(&wallet, &mint);
                pool.mint_tokens(&mint, &account, STARTING_BALANCE);
            }
        }

        pool
    }

    /// A pool that already exists and charges the default fees.
    pub fn opened() -> Self {
        Self::opened_under(TOKEN_PROGRAM_ID)
    }

    pub fn opened_under(token_program: Pubkey) -> Self {
        let mut pool = Self::new_under(token_program);
        pool.initialize(FEE_BPS, PROTOCOL_FEE_BPS).unwrap();
        pool
    }

    /// A pool that already holds `x` and `y` of liquidity.
    pub fn funded(x: u64, y: u64) -> Self {
        Self::funded_under(TOKEN_PROGRAM_ID, x, y)
    }

    pub fn funded_under(token_program: Pubkey, x: u64, y: u64) -> Self {
        let mut pool = Self::opened_under(token_program);
        let lp = pool.lp();
        pool.deposit(&lp, x, y, 0).unwrap();
        pool
    }

    pub fn admin(&self) -> Keypair {
        self.admin.insecure_clone()
    }

    pub fn lp(&self) -> Keypair {
        self.lp.insecure_clone()
    }

    pub fn trader(&self) -> Keypair {
        self.trader.insecure_clone()
    }

    /// Sends one transaction, paid for and signed by `signers[0]`. Expires
    /// the blockhash first, or two identical transactions in a row collapse
    /// into one and the second comes back AlreadyProcessed.
    pub fn send(&mut self, instructions: &[Instruction], signers: &[&Keypair]) -> TxResult {
        self.svm.expire_blockhash();
        let payer = signers[0].pubkey();
        let blockhash = self.svm.latest_blockhash();
        let message = Message::new_with_blockhash(instructions, Some(&payer), &blockhash);
        let transaction =
            VersionedTransaction::try_new(VersionedMessage::Legacy(message), signers).unwrap();

        self.svm.send_transaction(transaction)
    }

    pub fn initialize(&mut self, fee_bps: u16, protocol_fee_bps: u16) -> TxResult {
        self.initialize_pair(self.mint_x, self.mint_y, fee_bps, protocol_fee_bps)
    }

    pub fn initialize_pair(
        &mut self,
        mint_x: Pubkey,
        mint_y: Pubkey,
        fee_bps: u16,
        protocol_fee_bps: u16,
    ) -> TxResult {
        self.initialize_pair_under(
            mint_x,
            mint_y,
            self.token_program,
            fee_bps,
            protocol_fee_bps,
        )
    }

    /// Same instruction with the token program chosen by the caller, for the
    /// tests that build a pool out of Token-2022 mints.
    pub fn initialize_pair_under(
        &mut self,
        mint_x: Pubkey,
        mint_y: Pubkey,
        token_program: Pubkey,
        fee_bps: u16,
        protocol_fee_bps: u16,
    ) -> TxResult {
        let instruction = self.build(
            amm::instruction::Initialize {
                seed: self.seed,
                fee_bps,
                protocol_fee_bps,
                authority: self.admin.pubkey(),
            },
            amm::accounts::Initialize {
                admin: self.admin.pubkey(),
                mint_x,
                mint_y,
                config: self.config,
                lp_mint: self.lp_mint,
                vault_x: self.ata(&self.config, &mint_x),
                vault_y: self.ata(&self.config, &mint_y),
                treasury_x: treasury(&self.config, &mint_x),
                treasury_y: treasury(&self.config, &mint_y),
                locked_lp: self.locked_lp,
                token_program,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                system_program: system_program::ID,
            },
        );

        let admin = self.admin();
        self.send(&[instruction], &[&admin])
    }

    pub fn deposit(&mut self, user: &Keypair, max_x: u64, max_y: u64, min_lp: u64) -> TxResult {
        let instruction = self.build(
            amm::instruction::Deposit {
                max_x,
                max_y,
                min_lp,
            },
            amm::accounts::Deposit {
                user: user.pubkey(),
                mint_x: self.mint_x,
                mint_y: self.mint_y,
                config: self.config,
                lp_mint: self.lp_mint,
                vault_x: self.vault_x,
                vault_y: self.vault_y,
                locked_lp: self.locked_lp,
                user_x: self.ata(&user.pubkey(), &self.mint_x),
                user_y: self.ata(&user.pubkey(), &self.mint_y),
                user_lp: self.ata(&user.pubkey(), &self.lp_mint),
                token_program: self.token_program,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                system_program: system_program::ID,
            },
        );

        self.send(&[instruction], &[user])
    }

    pub fn withdraw(&mut self, user: &Keypair, lp_tokens: u64, min_x: u64, min_y: u64) -> TxResult {
        let instruction = self.build(
            amm::instruction::Withdraw {
                lp_tokens,
                min_x,
                min_y,
            },
            amm::accounts::Withdraw {
                user: user.pubkey(),
                mint_x: self.mint_x,
                mint_y: self.mint_y,
                config: self.config,
                lp_mint: self.lp_mint,
                vault_x: self.vault_x,
                vault_y: self.vault_y,
                user_x: self.ata(&user.pubkey(), &self.mint_x),
                user_y: self.ata(&user.pubkey(), &self.mint_y),
                user_lp: self.ata(&user.pubkey(), &self.lp_mint),
                token_program: self.token_program,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                system_program: system_program::ID,
            },
        );

        self.send(&[instruction], &[user])
    }

    pub fn swap(
        &mut self,
        user: &Keypair,
        mint_in: Pubkey,
        mint_out: Pubkey,
        amount_in: u64,
        min_amount_out: u64,
    ) -> TxResult {
        let vault_in = self.ata(&self.config, &mint_in);
        let vault_out = self.ata(&self.config, &mint_out);
        self.swap_from(
            user,
            mint_in,
            mint_out,
            vault_in,
            vault_out,
            amount_in,
            min_amount_out,
        )
    }

    /// Same instruction with the two vaults spelled out, so a test can point
    /// one of them at an account it should not reach.
    #[allow(clippy::too_many_arguments)]
    pub fn swap_from(
        &mut self,
        user: &Keypair,
        mint_in: Pubkey,
        mint_out: Pubkey,
        vault_in: Pubkey,
        vault_out: Pubkey,
        amount_in: u64,
        min_amount_out: u64,
    ) -> TxResult {
        let instruction = self.build(
            amm::instruction::Swap {
                amount_in,
                min_amount_out,
            },
            amm::accounts::Swap {
                user: user.pubkey(),
                mint_in,
                mint_out,
                config: self.config,
                vault_in,
                vault_out,
                treasury_in: treasury(&self.config, &mint_in),
                user_in: self.ata(&user.pubkey(), &mint_in),
                user_out: self.ata(&user.pubkey(), &mint_out),
                token_program: self.token_program,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                system_program: system_program::ID,
            },
        );

        self.send(&[instruction], &[user])
    }

    /// Opens a second pool on the same two mints under another seed, and
    /// gives back its config and its two vaults.
    pub fn open_sibling_pool(&mut self, seed: u64) -> (Pubkey, Pubkey, Pubkey) {
        let config = Pubkey::find_program_address(&[CONFIG_SEED, &seed.to_le_bytes()], &amm::ID).0;
        let lp_mint = Pubkey::find_program_address(&[LP_MINT_SEED, config.as_ref()], &amm::ID).0;
        let (vault_x, vault_y) = (
            self.ata(&config, &self.mint_x),
            self.ata(&config, &self.mint_y),
        );

        let instruction = self.build(
            amm::instruction::Initialize {
                seed,
                fee_bps: FEE_BPS,
                protocol_fee_bps: PROTOCOL_FEE_BPS,
                authority: self.admin.pubkey(),
            },
            amm::accounts::Initialize {
                admin: self.admin.pubkey(),
                mint_x: self.mint_x,
                mint_y: self.mint_y,
                config,
                lp_mint,
                vault_x,
                vault_y,
                treasury_x: treasury(&config, &self.mint_x),
                treasury_y: treasury(&config, &self.mint_y),
                locked_lp: self.ata(&config, &lp_mint),
                token_program: self.token_program,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                system_program: system_program::ID,
            },
        );

        let admin = self.admin();
        self.send(&[instruction], &[&admin]).unwrap();

        (config, vault_x, vault_y)
    }

    pub fn set_locked(&mut self, authority: &Keypair, locked: bool) -> TxResult {
        let instruction = self.build(
            amm::instruction::SetLocked { locked },
            amm::accounts::SetLocked {
                authority: authority.pubkey(),
                config: self.config,
            },
        );

        self.send(&[instruction], &[authority])
    }

    pub fn withdraw_fees(&mut self, authority: &Keypair, mint: Pubkey, amount: u64) -> TxResult {
        let source = treasury(&self.config, &mint);
        self.withdraw_fees_from(authority, mint, source, amount)
    }

    /// Same instruction with the source account chosen by the caller, so a
    /// test can try to point it somewhere it should not reach.
    pub fn withdraw_fees_from(
        &mut self,
        authority: &Keypair,
        mint: Pubkey,
        source: Pubkey,
        amount: u64,
    ) -> TxResult {
        let instruction = self.build(
            amm::instruction::WithdrawFees { amount },
            amm::accounts::WithdrawFees {
                authority: authority.pubkey(),
                mint,
                config: self.config,
                treasury: source,
                destination: self.ata(&authority.pubkey(), &mint),
                token_program: self.token_program,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                system_program: system_program::ID,
            },
        );

        self.send(&[instruction], &[authority])
    }

    pub fn config_state(&self) -> Config {
        let account = self
            .svm
            .get_account(&self.config)
            .expect("config not found");
        Config::try_deserialize(&mut account.data.as_slice()).unwrap()
    }

    /// The associated token address under this pool's token program.
    pub fn ata(&self, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        ata_under(owner, mint, &self.token_program)
    }

    pub fn balance(&self, token_account: &Pubkey) -> u64 {
        self.try_balance(token_account)
            .expect("token account not found")
    }

    pub fn try_balance(&self, token_account: &Pubkey) -> Option<u64> {
        let account = self.svm.get_account(token_account)?;
        if account.owner != TOKEN_PROGRAM_ID && account.owner != TOKEN_2022_PROGRAM_ID {
            return None;
        }
        Some(
            StateWithExtensions::<TokenAccountState>::unpack(&account.data)
                .unwrap()
                .base
                .amount,
        )
    }

    pub fn lp_supply(&self) -> u64 {
        self.mint_state(&self.lp_mint).supply
    }

    /// Product of the two vault balances, the value the swap invariant is
    /// supposed to protect.
    pub fn invariant(&self) -> u128 {
        (self.balance(&self.vault_x) as u128) * (self.balance(&self.vault_y) as u128)
    }

    pub fn build(&self, data: impl InstructionData, accounts: impl ToAccountMetas) -> Instruction {
        Instruction::new_with_bytes(amm::ID, &data.data(), accounts.to_account_metas(None))
    }

    pub fn create_mint(&mut self, decimals: u8) -> Pubkey {
        self.create_mint_owned_by(decimals, self.token_program)
    }

    /// A mint under a token program of the caller's choosing, so a test can
    /// build a pool that mixes SPL Token and Token-2022.
    pub fn create_mint_owned_by(&mut self, decimals: u8, token_program: Pubkey) -> Pubkey {
        let mint = Keypair::new();
        let admin = self.admin();
        let rent = self
            .svm
            .minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN);

        let instructions = [
            system_instruction::create_account(
                &admin.pubkey(),
                &mint.pubkey(),
                rent,
                spl_token::state::Mint::LEN as u64,
                &token_program,
            ),
            initialize_mint2(&token_program, &mint.pubkey(), &admin.pubkey(), decimals),
        ];
        self.send(&instructions, &[&admin, &mint]).unwrap();

        mint.pubkey()
    }

    /// A Token-2022 mint that charges a transfer fee, which is the kind of
    /// mint the pool has to turn away: less arrives in a vault than the curve
    /// was priced against.
    pub fn create_mint_with_transfer_fee(&mut self, decimals: u8) -> Pubkey {
        use anchor_spl::token_2022::spl_token_2022::{
            extension::{transfer_fee::instruction::initialize_transfer_fee_config, ExtensionType},
            state::Mint as Mint2022,
        };

        let mint = Keypair::new();
        let admin = self.admin();
        let len = ExtensionType::try_calculate_account_len::<Mint2022>(&[
            ExtensionType::TransferFeeConfig,
        ])
        .unwrap();
        let rent = self.svm.minimum_balance_for_rent_exemption(len);

        let instructions = [
            system_instruction::create_account(
                &admin.pubkey(),
                &mint.pubkey(),
                rent,
                len as u64,
                &TOKEN_2022_PROGRAM_ID,
            ),
            initialize_transfer_fee_config(
                &TOKEN_2022_PROGRAM_ID,
                &mint.pubkey(),
                Some(&admin.pubkey()),
                Some(&admin.pubkey()),
                100,
                u64::MAX,
            )
            .unwrap(),
            initialize_mint2(
                &TOKEN_2022_PROGRAM_ID,
                &mint.pubkey(),
                &admin.pubkey(),
                decimals,
            ),
        ];
        self.send(&instructions, &[&admin, &mint]).unwrap();

        mint.pubkey()
    }

    pub fn create_ata(&mut self, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        let admin = self.admin();
        let instruction =
            spl_associated_token_account::instruction::create_associated_token_account(
                &admin.pubkey(),
                owner,
                mint,
                &self.token_program,
            );
        self.send(&[instruction], &[&admin]).unwrap();

        self.ata(owner, mint)
    }

    /// Sends tokens straight into an account the pool owns, the way anyone
    /// can, without going through an instruction.
    pub fn donate(&mut self, from: &Keypair, mint: &Pubkey, destination: &Pubkey, amount: u64) {
        let instruction = transfer_checked(
            &self.token_program,
            &self.ata(&from.pubkey(), mint),
            mint,
            destination,
            &from.pubkey(),
            amount,
            self.mint_decimals(mint),
        );
        self.send(&[instruction], &[from]).unwrap();
    }

    /// A bare token transfer, signed by whoever the caller says, for the
    /// tests that try to move tokens without going through the program.
    pub fn raw_transfer(
        &mut self,
        authority: &Keypair,
        mint: &Pubkey,
        source: &Pubkey,
        destination: &Pubkey,
        amount: u64,
    ) -> TxResult {
        let instruction = transfer_checked(
            &self.token_program,
            source,
            mint,
            destination,
            &authority.pubkey(),
            amount,
            self.mint_decimals(mint),
        );
        self.send(&[instruction], &[authority])
    }

    pub fn mint_decimals(&self, mint: &Pubkey) -> u8 {
        self.mint_state(mint).decimals
    }

    fn mint_state(&self, mint: &Pubkey) -> MintState {
        let account = self.svm.get_account(mint).expect("mint not found");
        StateWithExtensions::<MintState>::unpack(&account.data)
            .unwrap()
            .base
    }

    pub fn mint_tokens(&mut self, mint: &Pubkey, destination: &Pubkey, amount: u64) {
        let admin = self.admin();
        let instruction = mint_to(
            &self.token_program,
            mint,
            destination,
            &admin.pubkey(),
            amount,
        );
        self.send(&[instruction], &[&admin]).unwrap();
    }
}

/// The SPL Token and Token-2022 builders encode these instructions the same
/// way, they just each refuse to build for the other's program id.
fn initialize_mint2(
    token_program: &Pubkey,
    mint: &Pubkey,
    authority: &Pubkey,
    decimals: u8,
) -> Instruction {
    if token_program == &TOKEN_2022_PROGRAM_ID {
        anchor_spl::token_2022::spl_token_2022::instruction::initialize_mint2(
            token_program,
            mint,
            authority,
            None,
            decimals,
        )
        .unwrap()
    } else {
        spl_token::instruction::initialize_mint2(token_program, mint, authority, None, decimals)
            .unwrap()
    }
}

fn mint_to(
    token_program: &Pubkey,
    mint: &Pubkey,
    destination: &Pubkey,
    authority: &Pubkey,
    amount: u64,
) -> Instruction {
    if token_program == &TOKEN_2022_PROGRAM_ID {
        anchor_spl::token_2022::spl_token_2022::instruction::mint_to(
            token_program,
            mint,
            destination,
            authority,
            &[],
            amount,
        )
        .unwrap()
    } else {
        spl_token::instruction::mint_to(token_program, mint, destination, authority, &[], amount)
            .unwrap()
    }
}

fn transfer_checked(
    token_program: &Pubkey,
    source: &Pubkey,
    mint: &Pubkey,
    destination: &Pubkey,
    authority: &Pubkey,
    amount: u64,
    decimals: u8,
) -> Instruction {
    if token_program == &TOKEN_2022_PROGRAM_ID {
        anchor_spl::token_2022::spl_token_2022::instruction::transfer_checked(
            token_program,
            source,
            mint,
            destination,
            authority,
            &[],
            amount,
            decimals,
        )
        .unwrap()
    } else {
        spl_token::instruction::transfer_checked(
            token_program,
            source,
            mint,
            destination,
            authority,
            &[],
            amount,
            decimals,
        )
        .unwrap()
    }
}

/// The associated token address for a mint under a given token program. The
/// program id is part of the seeds, so an SPL Token account and a Token-2022
/// account for the same owner and mint do not share an address.
pub fn ata_under(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    get_associated_token_address_with_program_id(owner, mint, token_program)
}

pub fn treasury(config: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[TREASURY_SEED, config.as_ref(), mint.as_ref()], &amm::ID).0
}

/// Asserts the transaction failed with one of the program's own errors.
pub fn assert_amm_error(result: TxResult, expected: AmmError) {
    assert_custom_error(result, u32::from(expected));
}

/// Asserts the transaction failed with a given error code, which for the
/// constraints Anchor generates itself is one of its 2xxx codes.
pub fn assert_custom_error(result: TxResult, expected_code: u32) {
    match result.expect_err("transaction was expected to fail").err {
        TransactionError::InstructionError(_, InstructionError::Custom(code)) => {
            assert_eq!(code, expected_code)
        }
        other => panic!("expected custom error {expected_code}, got {other:?}"),
    }
}
