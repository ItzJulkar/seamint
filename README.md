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
seamint eth mainnet gas-fee   Set Ethereum mainnet gas fee level (1=slow, 2=medium, 3=fast)
seamint chain rpc             Show the current RPC chain and select a network
                              (1 = Ink, 2 = Robinhood, 3 = Ethereum); writes RPC_URL to .env
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
| `RECIPIENT_FORWARD` | `false` | `true` = forward every minted NFT to `RECIPIENT_ADDRESS`; `false` = each minting wallet keeps its own NFT (self-funded default). Sponsored mode always forwards. |
| `RECIPIENT_ADDRESS` | — | NFT forward target (when `RECIPIENT_FORWARD=true`) and the withdrawal destination for `mint --withdraw`. Must differ from the executor. |
| `SPONSOR_KEY` | — | Pays outer gas in sponsored mode; also deployment, funding, undelegation. Fallback recipient. |
| `SPONSORED_EXECUTOR_ADDRESS` | — | Per-sponsor deterministic executor address (from `deploy-executor`), identical across supported chains. |
| `SPONSORED_OPERATION_DEADLINE_SECONDS` | `120` | Wallet mint-signature validity window (`30-3600`). |
| `FEE_AUTOMATIC` | `true` | Auto-estimate EIP-1559 fees; set `false` for manual `MAX_FEE_PER_GAS_GWEI` / `MAX_PRIORITY_FEE_PER_GAS_GWEI`. |
| `GAS_FEE_LEVEL` | chain default | Optional gas aggressiveness level (`slow`/`medium`/`fast`). When unset: fast on Robinhood/Ink, slow on Ethereum mainnet, medium elsewhere. On Ethereum it maps to the real Etherscan gas-tracker value; on cheap chains the RPC's real estimate. Set via `seamint eth mainnet gas-fee`. |
| `MAX_FEE_PER_GAS_GWEI` | — | Manual max fee per gas in gwei (`FEE_AUTOMATIC=false`). |
| `MAX_PRIORITY_FEE_PER_GAS_GWEI` | — | Manual priority fee per gas in gwei (`FEE_AUTOMATIC=false`). |
| `GAS_LIMIT` | `300000` | Mint transaction gas limit. |
| `REPLACEMENT_BUMP_BPS` | `11250` | Replacement fee bump in basis points (`11000-20000`). EVM nodes reject a same-nonce replacement unless fees rise by >= 10% (1000 bps), so the lower bound is 11000. |
| `SCHEDULE_REFRESH_INTERVAL_SECONDS` | `600` | Phase metadata + eligibility refresh cadence during long schedules (`10-86400`). |
| `TRANSACTION_MAX_ATTEMPTS` | `3` | Same-nonce replacement attempts (`1-10`). |
| `PENDING_TIMEOUT_SECONDS` | `20` | Pending-tx timeout before replacement (`1-86400`). |
| `RECEIPT_POLL_BASE_DELAY_MS` | `50` | Initial receipt-poll delay, exponential to the max (`50-60000`). |
| `RECEIPT_POLL_MAX_DELAY_MS` | `2000` | Upper bound for receipt polling (`50-60000`). |
| `OPENSEA_REQUEST_TIMEOUT_MS` | `10000` | OpenSea request timeout (`100-120000`). |
| `ELIGIBILITY_REQUEST_TIMEOUT_MS` | `5000` | Eligibility request timeout (`100-120000`). |
| `OPENSEA_MAX_ATTEMPTS` | `3` | OpenSea transport retry ceiling (`1-10`). |
| `OPENSEA_RETRY_INTERVAL_MS` | `50` | Fixed interval between OpenSea retries (`50-30000`). |
| `OPENSEA_CALLDATA_MAX_ATTEMPTS` | `40` | T-2 calldata retry ceiling for not-ready/transient actions (`1-1000`). |

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
  Wallets that succeed sign a separate safe-transfer only when
  `RECIPIENT_FORWARD=true`.

Recipient mode can be switched with a single command without editing `.env`:

- `seamint multi wallet recipient on` — each minting wallet keeps its own NFT
  (self-funded default). Optional `--recipient <address>` stores a withdrawal
  address for later.
- `seamint multi wallet recipient off [--recipient <address>]` — every NFT is
  forwarded to `RECIPIENT_ADDRESS`.
- `seamint multi wallet recipient status` — show the current mode and address.

Keep-own (`on`) is only available in self-funded mode: the sponsored EIP-7702
executor always forwards the NFT to the recipient, so sponsored minting
requires `RECIPIENT_FORWARD=true`. In keep-own mode `RECIPIENT_ADDRESS` is still
used as the withdrawal destination for `mint --withdraw`.

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
