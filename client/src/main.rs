//! Runs one pool through its whole life against a real cluster: two fresh
//! mints, a pool, a deposit, a swap, a fee withdrawal and an exit.
//!
//! Amounts and expected results match the LiteSVM suite, so the cluster and
//! the tests have to agree on the arithmetic or this fails.
//!
//! ```sh
//! cargo run --manifest-path client/Cargo.toml -- http://127.0.0.1:8899
//! cargo run --manifest-path client/Cargo.toml -- https://api.devnet.solana.com
//! ```

use {
    amm::constants::*,
    anchor_lang::solana_program::{
        instruction::Instruction, program_pack::Pack, pubkey::Pubkey, system_instruction,
        system_program,
    },
    anchor_lang::{InstructionData, ToAccountMetas},
    anchor_spl::{
        associated_token::{
            get_associated_token_address_with_program_id, spl_associated_token_account,
            ID as ASSOCIATED_TOKEN_PROGRAM_ID,
        },
        token::{spl_token, ID as TOKEN_PROGRAM_ID},
    },
    solana_client::rpc_client::RpcClient,
    solana_commitment_config::CommitmentConfig,
    solana_keypair::{read_keypair_file, Keypair},
    solana_signature::Signature,
    solana_signer::Signer,
    solana_transaction::Transaction,
    std::{
        env,
        error::Error,
        time::{SystemTime, UNIX_EPOCH},
    },
};

const DECIMALS_X: u8 = 6;
const DECIMALS_Y: u8 = 9;
const DEPOSIT_X: u64 = 100_000_000_000;
const DEPOSIT_Y: u64 = 400_000_000_000;
const SWAP_IN: u64 = 1_000_000_000;
/// What the curve pays for `SWAP_IN` on a 1e11 / 4e11 pool at 0.3%.
const EXPECTED_OUT: u64 = 3_948_632_137;
/// A fifth of the 3_000_000 fee.
const EXPECTED_PROTOCOL_FEE: u64 = 600_000;
const FEE_BPS: u16 = 30;
const PROTOCOL_FEE_BPS: u16 = 2_000;

struct Cluster {
    rpc: RpcClient,
    explorer_suffix: &'static str,
}

impl Cluster {
    fn new(url: String) -> Self {
        let explorer_suffix = if url.contains("devnet") {
            "?cluster=devnet"
        } else if url.contains("testnet") {
            "?cluster=testnet"
        } else if url.contains("127.0.0.1") || url.contains("localhost") {
            "?cluster=custom"
        } else {
            ""
        };

        Self {
            rpc: RpcClient::new_with_commitment(url, CommitmentConfig::confirmed()),
            explorer_suffix,
        }
    }

    fn send(
        &self,
        label: &str,
        instructions: &[Instruction],
        payer: &Keypair,
        signers: &[&Keypair],
    ) -> Result<Signature, Box<dyn Error>> {
        let blockhash = self.rpc.get_latest_blockhash()?;
        let transaction = Transaction::new_signed_with_payer(
            instructions,
            Some(&payer.pubkey()),
            signers,
            blockhash,
        );
        let signature = self.rpc.send_and_confirm_transaction(&transaction)?;
        println!(
            "  {label:<16} https://explorer.solana.com/tx/{signature}{}",
            self.explorer_suffix
        );

        Ok(signature)
    }

    fn token_balance(&self, account: &Pubkey) -> Result<u64, Box<dyn Error>> {
        Ok(self
            .rpc
            .get_token_account_balance(account)?
            .amount
            .parse::<u64>()?)
    }
}

fn ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    get_associated_token_address_with_program_id(owner, mint, &TOKEN_PROGRAM_ID)
}

fn treasury(config: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[TREASURY_SEED, config.as_ref(), mint.as_ref()], &amm::ID).0
}

fn instruction(data: impl InstructionData, accounts: impl ToAccountMetas) -> Instruction {
    Instruction::new_with_bytes(amm::ID, &data.data(), accounts.to_account_metas(None))
}

fn create_mint(cluster: &Cluster, payer: &Keypair, decimals: u8) -> Result<Pubkey, Box<dyn Error>> {
    let mint = Keypair::new();
    let rent = cluster
        .rpc
        .get_minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN)?;

    cluster.send(
        "mint",
        &[
            system_instruction::create_account(
                &payer.pubkey(),
                &mint.pubkey(),
                rent,
                spl_token::state::Mint::LEN as u64,
                &TOKEN_PROGRAM_ID,
            ),
            spl_token::instruction::initialize_mint2(
                &TOKEN_PROGRAM_ID,
                &mint.pubkey(),
                &payer.pubkey(),
                None,
                decimals,
            )?,
        ],
        payer,
        &[payer, &mint],
    )?;

    Ok(mint.pubkey())
}

fn main() -> Result<(), Box<dyn Error>> {
    let url = env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:8899".to_string());
    let wallet = env::args().nth(2).unwrap_or_else(|| {
        format!(
            "{}/.config/solana/id.json",
            env::var("HOME").unwrap_or_default()
        )
    });

    let cluster = Cluster::new(url.clone());
    let payer = read_keypair_file(&wallet)?;
    let seed = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    println!("cluster  {url}");
    println!("program  {}", amm::ID);
    println!("payer    {}", payer.pubkey());
    println!("seed     {seed}\n");

    println!("setting up two mints and funding the wallet");
    let mint_x = create_mint(&cluster, &payer, DECIMALS_X)?;
    let mint_y = create_mint(&cluster, &payer, DECIMALS_Y)?;
    let (user_x, user_y) = (ata(&payer.pubkey(), &mint_x), ata(&payer.pubkey(), &mint_y));
    cluster.send(
        "fund",
        &[
            spl_associated_token_account::instruction::create_associated_token_account(
                &payer.pubkey(),
                &payer.pubkey(),
                &mint_x,
                &TOKEN_PROGRAM_ID,
            ),
            spl_associated_token_account::instruction::create_associated_token_account(
                &payer.pubkey(),
                &payer.pubkey(),
                &mint_y,
                &TOKEN_PROGRAM_ID,
            ),
            spl_token::instruction::mint_to(
                &TOKEN_PROGRAM_ID,
                &mint_x,
                &user_x,
                &payer.pubkey(),
                &[],
                DEPOSIT_X * 2,
            )?,
            spl_token::instruction::mint_to(
                &TOKEN_PROGRAM_ID,
                &mint_y,
                &user_y,
                &payer.pubkey(),
                &[],
                DEPOSIT_Y * 2,
            )?,
        ],
        &payer,
        &[&payer],
    )?;

    let config = Pubkey::find_program_address(&[CONFIG_SEED, &seed.to_le_bytes()], &amm::ID).0;
    let lp_mint = Pubkey::find_program_address(&[LP_MINT_SEED, config.as_ref()], &amm::ID).0;
    let (vault_x, vault_y) = (ata(&config, &mint_x), ata(&config, &mint_y));
    let (treasury_x, treasury_y) = (treasury(&config, &mint_x), treasury(&config, &mint_y));
    let user_lp = ata(&payer.pubkey(), &lp_mint);

    println!("\nrunning the pool");
    cluster.send(
        "initialize",
        &[instruction(
            amm::instruction::Initialize {
                seed,
                fee_bps: FEE_BPS,
                protocol_fee_bps: PROTOCOL_FEE_BPS,
                authority: payer.pubkey(),
            },
            amm::accounts::Initialize {
                admin: payer.pubkey(),
                mint_x,
                mint_y,
                config,
                lp_mint,
                vault_x,
                vault_y,
                treasury_x,
                treasury_y,
                locked_lp: ata(&config, &lp_mint),
                token_program: TOKEN_PROGRAM_ID,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                system_program: system_program::ID,
            },
        )],
        &payer,
        &[&payer],
    )?;

    cluster.send(
        "deposit",
        &[instruction(
            amm::instruction::Deposit {
                max_x: DEPOSIT_X,
                max_y: DEPOSIT_Y,
                min_lp: 0,
            },
            amm::accounts::Deposit {
                user: payer.pubkey(),
                mint_x,
                mint_y,
                config,
                lp_mint,
                vault_x,
                vault_y,
                locked_lp: ata(&config, &lp_mint),
                user_x,
                user_y,
                user_lp,
                token_program: TOKEN_PROGRAM_ID,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                system_program: system_program::ID,
            },
        )],
        &payer,
        &[&payer],
    )?;

    let before_y = cluster.token_balance(&user_y)?;
    let swap_signature = cluster.send(
        "swap",
        &[instruction(
            amm::instruction::Swap {
                amount_in: SWAP_IN,
                min_amount_out: EXPECTED_OUT,
            },
            amm::accounts::Swap {
                user: payer.pubkey(),
                mint_in: mint_x,
                mint_out: mint_y,
                config,
                vault_in: vault_x,
                vault_out: vault_y,
                treasury_in: treasury_x,
                user_in: user_x,
                user_out: user_y,
                token_program: TOKEN_PROGRAM_ID,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                system_program: system_program::ID,
            },
        )],
        &payer,
        &[&payer],
    )?;
    let swapped = cluster.token_balance(&user_y)? - before_y;

    cluster.send(
        "withdraw_fees",
        &[instruction(
            amm::instruction::WithdrawFees {
                amount: EXPECTED_PROTOCOL_FEE,
            },
            amm::accounts::WithdrawFees {
                authority: payer.pubkey(),
                mint: mint_x,
                config,
                treasury: treasury_x,
                destination: user_x,
                token_program: TOKEN_PROGRAM_ID,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                system_program: system_program::ID,
            },
        )],
        &payer,
        &[&payer],
    )?;

    let held = cluster.token_balance(&user_lp)?;
    cluster.send(
        "withdraw",
        &[instruction(
            amm::instruction::Withdraw {
                lp_tokens: held,
                min_x: 0,
                min_y: 0,
            },
            amm::accounts::Withdraw {
                user: payer.pubkey(),
                mint_x,
                mint_y,
                config,
                lp_mint,
                vault_x,
                vault_y,
                user_x,
                user_y,
                user_lp,
                token_program: TOKEN_PROGRAM_ID,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                system_program: system_program::ID,
            },
        )],
        &payer,
        &[&payer],
    )?;

    println!("\nchecking the numbers against the curve");
    let lp_supply = cluster
        .rpc
        .get_token_supply(&lp_mint)?
        .amount
        .parse::<u64>()?;

    assert_eq!(
        swapped, EXPECTED_OUT,
        "the cluster should price the swap exactly as the curve tests do"
    );
    assert_eq!(
        cluster.token_balance(&treasury_x)?,
        0,
        "the fee that was collected is the fee that was withdrawn"
    );
    assert_eq!(
        cluster.token_balance(&treasury_y)?,
        0,
        "nothing was traded into the y side"
    );
    assert_eq!(
        lp_supply, MINIMUM_LIQUIDITY,
        "the only LP tokens left are the locked ones"
    );
    assert_eq!(
        cluster.token_balance(&user_lp)?,
        0,
        "the position is closed"
    );

    println!("  swap paid          {swapped} of mint y, the curve said {EXPECTED_OUT}");
    println!("  protocol fee       {EXPECTED_PROTOCOL_FEE} of mint x, collected and withdrawn");
    println!("  lp supply left     {lp_supply}, the locked minimum and nothing else");
    println!("\npool   {config}");
    println!(
        "swap   https://explorer.solana.com/tx/{swap_signature}{}",
        cluster.explorer_suffix
    );
    println!("\nall of it agrees with the tests");

    Ok(())
}
