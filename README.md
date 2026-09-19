# AMM

Constant product AMM for Solana, written with Anchor. Two vaults hold the
reserves, an LP mint tracks who owns what share of them, and every swap pays a
fee split between the liquidity providers and a protocol treasury.

The curve math is in [`curve.rs`](programs/amm/src/curve.rs) instead of a
dependency, so the rounding and overflow behaviour is auditable here.

## Instructions

| Instruction | Who | What it does |
| --- | --- | --- |
| `initialize(seed, fee_bps, protocol_fee_bps, authority)` | anyone | Creates the config, LP mint, vaults and treasuries |
| `deposit(max_x, max_y, min_lp)` | anyone | Adds liquidity at the current ratio |
| `withdraw(lp_tokens, min_x, min_y)` | LP holder | Burns LP tokens and pays out both sides |
| `swap(amount_in, min_amount_out)` | anyone | Trades one pool mint for the other |
| `set_locked(locked)` | authority | Stops deposits and swaps, never withdrawals |
| `withdraw_fees(amount)` | authority | Moves fees out of one treasury |

Every user instruction takes a bound from the caller and fails rather than
fill at a worse price. `swap` has no direction flag: the caller passes the two
mints and the config checks they are this pool's pair in either order.

A pool is one `Config` and the five token accounts it owns:

| Account | Address |
| --- | --- |
| `config` | `["config", seed]` |
| `lp_mint` | `["lp_mint", config]` |
| `vault_x` / `vault_y` | associated token accounts of `config` |
| `treasury_x` / `treasury_y` | `["treasury", config, mint]` |
| `locked_lp` | associated token account of `config` for `lp_mint` |

`config` signs every token CPI. `seed` is a caller-chosen `u64`, so one mint
pair can have several pools.

## The math

The invariant is `x * y = k`. A swap takes the fee off the input, rounded up,
and prices what remains against the invariant, rounded down:

```text
fee        = ceil(amount_in * fee_bps / 10_000)
cut        = fee * protocol_fee_bps / 10_000        -> treasury
net        = amount_in - fee
amount_out = reserve_out * net / (reserve_in + net)
```

Only the protocol cut leaves the pool, so `k` never falls.

The first deposit sets the price and mints `sqrt(x * y)`, less 1,000 LP tokens
that stay locked. Later deposits follow the smaller side and round the other
up, so a depositor can pay one base unit over the ratio and never one under.
Withdrawals round down on both sides. Every product widens to `u128` before
dividing, so two `u64::MAX` reserves cannot overflow the intermediate.
