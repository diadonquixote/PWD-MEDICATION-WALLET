# MediWallet

> A restricted-spend blockchain wallet on Stellar — government medical grants for Persons with Disabilities (PWDs), spendable only at accredited hospitals and pharmacies.

---

## Problem

A PWD beneficiary in Bacoor, Cavite receives a monthly medical allowance from the Local Government Unit (LGU), but the cash is easily redirected to non-medical expenses by family members or lost entirely — leaving the PWD without medication and the government with no auditability on how public funds were used.

## Solution

MediWallet issues each registered PWD an on-chain escrow wallet funded by the LGU. A Soroban smart contract enforces that tokens can only be transferred to addresses whitelisted by the government (hospitals, pharmacies). The restriction is enforced at the protocol level — no app-side bypass is possible. Every disbursement and payment is publicly verifiable on Stellar Horizon.

---

## Stellar Features Used

| Feature | Purpose |
|---|---|
| Soroban smart contracts | Whitelist enforcement, escrow logic, expiry/clawback |
| Stellar Asset Contract (SAC) | USDC or custom MediToken as the benefit currency |
| Clawback pattern | Government recovers unspent funds after grant window expires |
| XLM / USDC transfers | Sub-cent transaction fees — viable for small medication payments |
| Trustlines | PWD wallet authorised to hold the restricted asset |

---

## Target Users

- **PWDs** — low-income individuals with government-issued disability certificates in Philippine municipalities
- **LGU health officers** — administer monthly medical grants and need tamper-proof disbursement records
- **Accredited pharmacies / hospitals** — receive payment instantly, no float, no fraud

---

## Core MVP Transaction Flow

```
LGU admin calls disburse(pwd_address, 5000, expiry_ledger)
  → tokens move from admin wallet into contract escrow
  → PWD balance updated on-chain

PWD scans QR code at Mercury Drug (whitelisted merchant)
  → PWD signs pay(pwd_address, pharmacy_address, 780)
  → contract checks: beneficiary? ✓  not expired? ✓  merchant whitelisted? ✓
  → 780 tokens transfer from escrow to pharmacy
  → Horizon tx confirmed in < 2 seconds
```

Demo time: **< 2 minutes** (register → disburse → pay → verify on Horizon).

---

## Prerequisites

| Tool | Version |
|---|---|
| Rust | `>= 1.74` |
| Soroban CLI | `>= 20.0.0` |
| `wasm32-unknown-unknown` target | `rustup target add wasm32-unknown-unknown` |

Install Soroban CLI:
```bash
cargo install --locked soroban-cli --features opt
```

---

## Build

```bash
soroban contract build
# Output: target/wasm32-unknown-unknown/release/mediwallet.wasm
```

---

## Test

```bash
cargo test
```

Expected output: 5 tests passing.

```
test test_happy_path_full_disbursement_and_payment     ... ok
test test_payment_to_non_whitelisted_merchant_is_rejected ... ok
test test_state_is_correct_after_payment               ... ok
test test_clawback_recovers_funds_after_expiry         ... ok
test test_payment_exceeding_balance_is_rejected        ... ok
```

---

## Deploy to Testnet

```bash
# Configure testnet identity
soroban keys generate --global admin --network testnet

# Fund with Friendbot
soroban keys fund admin --network testnet

# Deploy contract
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/mediwallet.wasm \
  --source admin \
  --network testnet
# Returns: CONTRACT_ID
```

---

## Sample CLI Invocations

Replace `<CONTRACT_ID>`, `<ADMIN_ADDRESS>`, `<TOKEN_ADDRESS>`, `<PWD_ADDRESS>`, `<PHARMACY_ADDRESS>` with real values.

**Initialize the contract:**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source admin \
  --network testnet \
  -- initialize \
  --admin <ADMIN_ADDRESS> \
  --token <TOKEN_ADDRESS>
```

**Register a PWD beneficiary:**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source admin \
  --network testnet \
  -- register_beneficiary \
  --pwd <PWD_ADDRESS>
```

**Whitelist a pharmacy:**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source admin \
  --network testnet \
  -- add_merchant \
  --merchant <PHARMACY_ADDRESS>
```

**Disburse monthly grant (5000 units, expires at ledger 500000):**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source admin \
  --network testnet \
  -- disburse \
  --pwd <PWD_ADDRESS> \
  --amount 5000 \
  --expiry_ledger 500000
```

**PWD pays pharmacy (780 units):**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source <PWD_KEYPAIR> \
  --network testnet \
  -- pay \
  --pwd <PWD_ADDRESS> \
  --merchant <PHARMACY_ADDRESS> \
  --amount 780
```

**Check PWD balance:**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- balance \
  --pwd <PWD_ADDRESS>
```

**Clawback expired funds:**
```bash
soroban contract invoke \
  --id <CONTRACT_ID> \
  --source admin \
  --network testnet \
  -- clawback \
  --pwd <PWD_ADDRESS>
```

---

## Project Structure

```
mediwallet/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs    ← Soroban contract
    └── test.rs   ← 5 unit tests
```

---

## Why This Wins

Stellar's sub-cent fees and 2-second finality make this viable for real ₱50–₱500 medication payments that would be uneconomical on any other chain. Soroban's authorization model means the spending restriction is cryptographically enforced — not just a UI gate — which is the trust guarantee government partners require.

---

## License

MIT © Kristina Diane Aviles