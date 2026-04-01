#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env,
};

// ── Shared test setup ─────────────────────────────────────────────────────────

struct Suite {
    env:          Env,
    client:       MediWalletContractClient<'static>,
    admin:        Address,
    token_client: token::Client<'static>,
    token_admin:  token::StellarAssetClient<'static>,
}

fn setup() -> Suite {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    // Deploy a minimal Stellar Asset Contract to act as the benefit token
    let token_addr = env.register_stellar_asset_contract(admin.clone());
    let token_client = token::Client::new(&env, &token_addr);
    let token_admin  = token::StellarAssetClient::new(&env, &token_addr);

    // Mint 1 000 000 units to admin so it can disburse grants
    token_admin.mint(&admin, &1_000_000_i128);

    // Deploy the MediWallet contract
    let contract_id = env.register_contract(None, MediWalletContract);
    let client = MediWalletContractClient::new(&env, &contract_id);

    // Allow contract to pull tokens from admin on disburse
    token_client.approve(&admin, &contract_id, &1_000_000_i128, &999_999);

    client.initialize(&admin, &token_addr);

    // Leak env lifetime so Suite fields can hold borrows — test-only pattern
    let env: Env = unsafe { core::mem::transmute(env) };
    let client: MediWalletContractClient<'static> = unsafe { core::mem::transmute(client) };
    let token_client: token::Client<'static> = unsafe { core::mem::transmute(token_client) };
    let token_admin: token::StellarAssetClient<'static> = unsafe { core::mem::transmute(token_admin) };

    Suite { env, client, admin, token_client, token_admin }
}

// ── Test 1: Happy path ────────────────────────────────────────────────────────
// Government disburses funds → PWD pays whitelisted pharmacy → balances correct

#[test]
fn test_happy_path_full_disbursement_and_payment() {
    let s = setup();

    let pwd      = Address::generate(&s.env);
    let pharmacy = Address::generate(&s.env);

    // LGU registers the PWD and whitelists the pharmacy
    s.client.register_beneficiary(&pwd);
    s.client.add_merchant(&pharmacy);

    // Government disburses ₱5 000 monthly grant (no expiry)
    s.client.disburse(&pwd, &5_000_i128, &0);
    assert_eq!(s.client.balance(&pwd), 5_000, "balance after disburse");

    // PWD buys medicine worth ₱780
    s.client.pay(&pwd, &pharmacy, &780_i128);

    // PWD balance reduced, pharmacy received the tokens
    assert_eq!(s.client.balance(&pwd),          4_220, "pwd balance after payment");
    assert_eq!(s.token_client.balance(&pharmacy),  780, "pharmacy received tokens");
}

// ── Test 2: Edge case ─────────────────────────────────────────────────────────
// Paying a non-whitelisted address (e.g. a grocery store) must be rejected

#[test]
#[should_panic(expected = "MerchantNotWhitelisted")]
fn test_payment_to_non_whitelisted_merchant_is_rejected() {
    let s = setup();

    let pwd          = Address::generate(&s.env);
    let grocery_store = Address::generate(&s.env); // NOT whitelisted

    s.client.register_beneficiary(&pwd);
    s.client.disburse(&pwd, &5_000_i128, &0);

    // This must panic — funds cannot leave the medical ecosystem
    s.client.pay(&pwd, &grocery_store, &500_i128);
}

// ── Test 3: State verification ────────────────────────────────────────────────
// After a payment, all storage slots reflect the correct updated state

#[test]
fn test_state_is_correct_after_payment() {
    let s = setup();

    let pwd      = Address::generate(&s.env);
    let hospital = Address::generate(&s.env);

    s.client.register_beneficiary(&pwd);
    s.client.add_merchant(&hospital);
    s.client.disburse(&pwd, &10_000_i128, &0);

    // Make two separate payments
    s.client.pay(&pwd, &hospital, &2_000_i128);
    s.client.pay(&pwd, &hospital, &3_500_i128);

    // On-chain state assertions
    assert_eq!(s.client.balance(&pwd),              4_500, "remaining balance");
    assert!(s.client.is_beneficiary(&pwd),                 "still a beneficiary");
    assert!(s.client.is_merchant(&hospital),               "still whitelisted");
    assert_eq!(s.token_client.balance(&hospital),   5_500, "hospital received both payments");
}

// ── Test 4: Expiry and clawback ───────────────────────────────────────────────
// After a grant expires the government can reclaim unspent funds

#[test]
fn test_clawback_recovers_funds_after_expiry() {
    let s = setup();

    let pwd = Address::generate(&s.env);
    s.client.register_beneficiary(&pwd);

    // Disburse with expiry at ledger sequence 100
    s.client.disburse(&pwd, &8_000_i128, &100);
    assert_eq!(s.client.balance(&pwd), 8_000, "balance before expiry");

    // Advance the ledger past the expiry point
    s.env.ledger().with_mut(|li| li.sequence_number = 101);

    let admin_before = s.token_client.balance(&s.admin);
    s.client.clawback(&pwd);

    // All unspent funds returned to government treasury
    assert_eq!(s.client.balance(&pwd),                           0, "pwd balance zeroed");
    assert_eq!(s.token_client.balance(&s.admin), admin_before + 8_000, "admin recovered funds");
}

// ── Test 5: Insufficient balance ─────────────────────────────────────────────
// PWD cannot spend more than their disbursed grant

#[test]
#[should_panic(expected = "InsufficientBalance")]
fn test_payment_exceeding_balance_is_rejected() {
    let s = setup();

    let pwd      = Address::generate(&s.env);
    let pharmacy = Address::generate(&s.env);

    s.client.register_beneficiary(&pwd);
    s.client.add_merchant(&pharmacy);

    // Only ₱1 000 disbursed
    s.client.disburse(&pwd, &1_000_i128, &0);

    // PWD tries to spend ₱9 999 — must fail
    s.client.pay(&pwd, &pharmacy, &9_999_i128);
}