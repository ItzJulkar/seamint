# seamint

A cross-platform Rust CLI for minting OpenSea-hosted (SeaDrop) NFT collections.
Single wallet, sponsored multi-wallet, or self-funded concurrent multi-wallet —
same engine, three ways to run it.

Built for drops where speed and correctness matter: it reads the real collection
state from OpenSea, authenticates every wallet, validates calldata locally, and
never signs anything until its own checks pass.

## Why this exists

OpenSea-hosted mints front-run the public sale with allowlist (WL) and FCFS
phases. Doing that by hand across multiple wallets means clicking through a
browser at T-0, which loses. This CLI does the T-10/T-2 preparation, captures
nonce/fees/balance/eligibility up front, fetches wallet-specific mint calldata,
and fires signed EIP-1559 transactions at the right moment.

It is a tool for people who already know what they are doing. Read the
disclaimer before you point it at real money.

## Modes

| Mode | Env | What happens |
| --- | --- | --- |
| Single wallet | `WALLET_KEY` | One wallet signs and submits its own mint. NFT stays in that wallet. |
| Sponsored multi-wallet | `WALLETS_FILE` + `SPONSORED=true` | Up to 25 wallets sign their own EIP-712 mint operations; a sponsor pays the outer gas via an EIP-7702 executor. NFTs forward to the recipient. |
| Self-funded multi-wallet | `WALLETS_FILE` + `SPONSORED=false` | Up to 10 wallets run concurrently, each paying its own mint value and gas. Failures don't stop the others. |

Sponsored mode deliberately picks `WALLETS_FILE` when both wallet sources are
configured — no silent ambiguity.

## Commands

```
seamint doctor              Validate wallet mode, RPC, and the local protocol boundary
seamint deploy-executor     Deploy + verify the deterministic per-sponsor executor
seamint mint                Run the mint (the actual event)
seamint mint --fund AMT     Fund every manifest wallet with AMT native tokens
seamint mint --withdraw     Withdraw each wallet's maximum safe balance
seamint mint --undelegate   Revoke EIP-7702 delegation for all wallets
seamint calldata --collection SLUG --wallets FILE [--token-id N]
                            Fetch + validate mint calldata without signing (dry run)
seamint wallets create --count N --quantity Q --output FILE
                            Generate a fresh wallet manifest (offline)
```

`seamint doctor` is the first thing you run after configuring — it checks the
whole local boundary before you trust it with a real mint.

## Quickstart

```sh
# 1. Build (release profile is tuned: LTO, stripped)
cargo build --release

# 2. Configure
cp .env.example .env          # then edit: RPC_URL, WALLET_KEY or WALLETS_FILE
cp wallets.example.json wallets.json   # only for multi-wallet mode

# 3. Generate a manifest (multi-wallet mode)
seamint wallets create --count 10 --quantity 1

# 4. Validate everything
seamint doctor

# 5. Fire when ready
seamint mint
```

## Configuration

A single `.env` drives everything. The important knobs:

| Variable | Default | Meaning |
| --- | --- | --- |
| `RPC_URL` | — | Chain RPC matching the collection. Must pass EIP-7702 + EIP-1153 probes for sponsored mode. |
| `WALLET_KEY` | — | Single-wallet mode: the one signing key. |
| `WALLETS_FILE` | — | Multi-wallet mode: path to the generated manifest. |
| `SPONSORED` | — | `true` = sponsored EIP-7702 mode, `false` = self-funded concurrent. |
| `RECIPIENT_ADDRESS` | — | Where minted NFTs go in multi-wallet mode. Must differ from the executor. |
| `SPONSOR_KEY` | — | Pays outer gas in sponsored mode; also deployment, funding, undelegation. Fallback recipient. |
| `SPONSORED_OPERATION_DEADLINE_SECONDS` | `120` | Wallet mint-signature validity window (30–3600). |
| `FEE_AUTOMATIC` | `true` | Auto-estimate EIP-1559 fees; set `false` for manual `MAX_FEE_PER_GAS_GWEI` / `MAX_PRIORITY_FEE_PER_GAS_GWEI`. |
| `GAS_LIMIT` | `300000` | Mint transaction gas limit. |
| `TRANSACTION_MAX_ATTEMPTS` | `3` | Same-nonce replacement attempts (1–10). |
| `PENDING_TIMEOUT_SECONDS` | `20` | Pending-tx timeout before replacement. |
| `REPLACEMENT_BUMP_BPS` | `11250` | Replacement fee bump in basis points. |
| `OPENSEA_REQUEST_TIMEOUT_MS` | `10000` | OpenSea request timeout. |
| `OPENSEA_CALLDATA_MAX_ATTEMPTS` | `40` | T-2 calldata retry ceiling for not-ready/transient actions. |
| `SCHEDULE_REFRESH_INTERVAL_SECONDS` | `600` | Phase metadata + eligibility refresh cadence during long schedules. |

### `.env` discovery

An uninstalled binary inside the project tree searches upward from the binary
location first (so a parent `.env` can't shadow the project file). An installed
`seamint` searches the launch directory and its parents.

## How the mint works

1. **T-10** — capture nonce, fees, balance, metadata, eligibility, local funding.
2. **T-2** — fetch and validate wallet-specific calldata; retry transient or
   locally inconsistent actions at a fixed interval. Nothing is signed until
   local validation passes.
3. **T-0** — sign and submit EIP-1559 transactions, track receipts, bump on
   pending timeouts with same-nonce replacements.

Multi-wallet specifics:

- **Sponsored** — executor runtime is verified live before use. All wallet
  actions are fetched in one aliased GraphQL request. Each wallet signs its
  exact EIP-712 mint op; the sponsor pays outer gas. Successful NFTs forward
  atomically to the recipient; failed wallets keep their mint value without
  undoing others. Run `seamint mint --undelegate` afterwards to revoke.
- **Self-funded** — during setup, the CLI computes mint value + max gas + fees
  locally and prompts to top up / recheck / skip underfunded wallets. Execution
  is concurrent and independent; receipts are verified and NFTs extracted.
  Wallets that succeed sign a separate safe-transfer when a recipient is set.

## Executor

Sponsored mode uses a deterministic per-sponsor executor deployed from
`contracts/SponsoredMintExecutor.sol` via Foundry. The deployment salt is
`keccak256("seamint/SponsoredMintExecutor/v1" || sponsor)`, so the executor
address is identical across supported chains for the same sponsor — and
namespaced away from any other tool using a different salt domain.

```
forge build --root contracts
seamint deploy-executor
```

The executor is **unaudited**. Verify it yourself before funding it with gas.

## Development

```sh
cargo test          # unit + integration (CLI surface, domain flow, fee policy)
cargo clippy        # pedantic lints on by default
forge test --root contracts
```

Rust toolchain is pinned via `rust-toolchain.toml` (edition 2024).

## Disclaimer

**Use this software entirely at your own risk.** It uses an unaudited EIP-7702
executor smart contract and OpenSea's private internal API, which may change,
become incompatible, or stop working at any time. Blockchain transactions are
irreversible and may result in loss of funds or digital assets.

The software is provided "as is" without warranties of any kind. To the maximum
extent permitted by law, the author and contributors will not be liable for any
direct, indirect, incidental, consequential, financial, technical, or other
loss arising from use of, inability to use, or reliance on this software.

## License

MIT — see [LICENSE](LICENSE).
