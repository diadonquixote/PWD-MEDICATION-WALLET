
Copy

#![no_std]
 
//! MediWallet — Restricted-spend PWD benefit wallet on Stellar
//! Government disburses funds that can ONLY be spent at whitelisted
//! hospitals and pharmacies. All logic is enforced on-chain via Soroban.
 
use soroban_sdk::{
    contract, contractimpl, contracttype,
    Address, Env, Symbol, token,
    panic_with_error,
};
 
// ── Error codes ──────────────────────────────────────────────────────────────
 
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized    = 1,
    NotAdmin              = 2,
    NotRegistered         = 3,
    AlreadyRegistered     = 4,
    MerchantNotWhitelisted = 5,
    InsufficientBalance   = 6,
    ZeroAmount            = 7,
    FundsExpired          = 8,
    NotExpiredYet         = 9,
}
 
impl soroban_sdk::TryFromVal<Env, soroban_sdk::Val> for Error {
    type Error = soroban_sdk::ConversionError;
    fn try_from_val(_env: &Env, v: &soroban_sdk::Val) -> Result<Self, Self::Error> {
        let u: u32 = soroban_sdk::TryFromVal::try_from_val(_env, v)?;
        match u {
            1 => Ok(Error::AlreadyInitialized),
            2 => Ok(Error::NotAdmin),
            3 => Ok(Error::NotRegistered),
            4 => Ok(Error::AlreadyRegistered),
            5 => Ok(Error::MerchantNotWhitelisted),
            6 => Ok(Error::InsufficientBalance),
            7 => Ok(Error::ZeroAmount),
            8 => Ok(Error::FundsExpired),
            9 => Ok(Error::NotExpiredYet),
            _ => Err(soroban_sdk::ConversionError),
        }
    }
}
 
// ── Storage keys ─────────────────────────────────────────────────────────────
 
#[contracttype]
pub enum DataKey {
    /// Address of the government / LGU admin account
    Admin,
    /// Stellar asset contract address (USDC or custom MediToken)
    Token,
    /// PWD address → i128 restricted balance held in escrow
    Balance(Address),
    /// PWD address → bool (registered beneficiary flag)
    Beneficiary(Address),
    /// Merchant address → bool (whitelisted hospital/pharmacy flag)
    Merchant(Address),
    /// PWD address → u32 ledger sequence after which funds expire (0 = no expiry)
    Expiry(Address),
}
 
// ── Contract ─────────────────────────────────────────────────────────────────
 
#[contract]
pub struct MediWalletContract;
 
#[contractimpl]
impl MediWalletContract {
 
    // ── Setup ─────────────────────────────────────────────────────────────────
 
    /// One-time initialization. Stores admin (LGU wallet) and the token asset
    /// address. Must be called immediately after deployment.
    pub fn initialize(env: Env, admin: Address, token: Address) {
        // Prevent re-initialization — critical for security
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
    }
 
    // ── Admin: beneficiary management ─────────────────────────────────────────
 
    /// LGU registers a PWD as an authorized fund recipient.
    /// Sets their on-chain balance to 0 and marks them as active.
    pub fn register_beneficiary(env: Env, pwd: Address) {
        Self::require_admin(&env);
        if env.storage().persistent().has(&DataKey::Beneficiary(pwd.clone())) {
            panic_with_error!(&env, Error::AlreadyRegistered);
        }
        // Mark as active beneficiary
        env.storage().persistent().set(&DataKey::Beneficiary(pwd.clone()), &true);
        // Initialize balance at zero
        env.storage().persistent().set(&DataKey::Balance(pwd.clone()), &0_i128);
 
        env.events().publish(
            (Symbol::new(&env, "beneficiary_added"),),
            pwd,
        );
    }
 
    /// LGU removes a PWD (deceased, fraud detected, etc.).
    /// Any remaining escrowed balance is returned to the admin treasury.
    pub fn deregister_beneficiary(env: Env, pwd: Address) {
        Self::require_admin(&env);
        Self::assert_beneficiary(&env, &pwd);
 
        // Return any unspent balance to admin before removing
        let remaining: i128 = env.storage().persistent()
            .get(&DataKey::Balance(pwd.clone()))
            .unwrap_or(0);
 
        if remaining > 0 {
            let admin = Self::admin(&env);
            token::Client::new(&env, &Self::token(&env))
                .transfer(&env.current_contract_address(), &admin, &remaining);
        }
 
        // Clean up all storage entries for this PWD
        env.storage().persistent().remove(&DataKey::Beneficiary(pwd.clone()));
        env.storage().persistent().remove(&DataKey::Balance(pwd.clone()));
        env.storage().persistent().remove(&DataKey::Expiry(pwd.clone()));
 
        env.events().publish(
            (Symbol::new(&env, "beneficiary_removed"),),
            pwd,
        );
    }
 
    // ── Admin: merchant whitelist ─────────────────────────────────────────────
 
    /// Add a hospital or pharmacy to the spending whitelist.
    /// Only whitelisted addresses can receive funds from PWD wallets.
    pub fn add_merchant(env: Env, merchant: Address) {
        Self::require_admin(&env);
        env.storage().persistent().set(&DataKey::Merchant(merchant.clone()), &true);
        env.events().publish((Symbol::new(&env, "merchant_added"),), merchant);
    }
 
    /// Remove a merchant from the whitelist (license expired, fraud, closure).
    pub fn remove_merchant(env: Env, merchant: Address) {
        Self::require_admin(&env);
        env.storage().persistent().remove(&DataKey::Merchant(merchant.clone()));
        env.events().publish((Symbol::new(&env, "merchant_removed"),), merchant);
    }
 
    // ── Admin: fund disbursement ──────────────────────────────────────────────
 
    /// Government disburses `amount` tokens into a PWD's restricted escrow.
    /// The tokens are pulled from the admin account into this contract.
    /// `expiry_ledger` = 0 means funds never expire.
    pub fn disburse(env: Env, pwd: Address, amount: i128, expiry_ledger: u32) {
        Self::require_admin(&env);
        Self::assert_beneficiary(&env, &pwd);
 
        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
 
        // Transfer tokens from admin into this escrow contract
        let admin = Self::admin(&env);
        token::Client::new(&env, &Self::token(&env))
            .transfer(&admin, &env.current_contract_address(), &amount);
 
        // Add to existing balance (supports multiple disbursements)
        let current: i128 = env.storage().persistent()
            .get(&DataKey::Balance(pwd.clone()))
            .unwrap_or(0);
        env.storage().persistent()
            .set(&DataKey::Balance(pwd.clone()), &(current + amount));
 
        // Set expiry if provided (e.g. 30-day grant window)
        if expiry_ledger > 0 {
            env.storage().persistent()
                .set(&DataKey::Expiry(pwd.clone()), &expiry_ledger);
        }
 
        env.events().publish(
            (Symbol::new(&env, "disbursed"), pwd),
            amount,
        );
    }
 
    // ── PWD: restricted payment ───────────────────────────────────────────────
 
    /// PWD pays a merchant. This is the core MVP transaction:
    ///   1. PWD signs the transaction (require_auth)
    ///   2. Contract verifies: beneficiary registered, not expired, merchant whitelisted
    ///   3. Deducts from escrow balance
    ///   4. Transfers tokens from contract to merchant
    /// Stellar enforces this — no app-side bypass is possible.
    pub fn pay(env: Env, pwd: Address, merchant: Address, amount: i128) {
        // Require PWD's own signature — prevents unauthorized spending
        pwd.require_auth();
 
        Self::assert_beneficiary(&env, &pwd);
 
        // Check if the grant window has expired
        let expiry: u32 = env.storage().persistent()
            .get(&DataKey::Expiry(pwd.clone()))
            .unwrap_or(0);
        if expiry > 0 && env.ledger().sequence() > expiry {
            panic_with_error!(&env, Error::FundsExpired);
        }
 
        // Enforce the merchant whitelist — core restriction logic
        let is_whitelisted: bool = env.storage().persistent()
            .get(&DataKey::Merchant(merchant.clone()))
            .unwrap_or(false);
        if !is_whitelisted {
            panic_with_error!(&env, Error::MerchantNotWhitelisted);
        }
 
        if amount <= 0 {
            panic_with_error!(&env, Error::ZeroAmount);
        }
 
        let balance: i128 = env.storage().persistent()
            .get(&DataKey::Balance(pwd.clone()))
            .unwrap_or(0);
 
        if balance < amount {
            panic_with_error!(&env, Error::InsufficientBalance);
        }
 
        // Debit PWD escrow
        env.storage().persistent()
            .set(&DataKey::Balance(pwd.clone()), &(balance - amount));
 
        // Credit merchant directly from contract escrow
        token::Client::new(&env, &Self::token(&env))
            .transfer(&env.current_contract_address(), &merchant, &amount);
 
        env.events().publish(
            (Symbol::new(&env, "payment"), pwd, merchant),
            amount,
        );
    }
 
    // ── Admin: expire and recover funds ──────────────────────────────────────
 
    /// Reclaim unspent funds after a grant expires.
    /// Protects public budget from permanently locked tokens.
    pub fn clawback(env: Env, pwd: Address) {
        Self::require_admin(&env);
        Self::assert_beneficiary(&env, &pwd);
 
        let expiry: u32 = env.storage().persistent()
            .get(&DataKey::Expiry(pwd.clone()))
            .unwrap_or(0);
 
        // Can only clawback after expiry ledger has passed
        if expiry == 0 || env.ledger().sequence() <= expiry {
            panic_with_error!(&env, Error::NotExpiredYet);
        }
 
        let balance: i128 = env.storage().persistent()
            .get(&DataKey::Balance(pwd.clone()))
            .unwrap_or(0);
 
        if balance > 0 {
            let admin = Self::admin(&env);
            token::Client::new(&env, &Self::token(&env))
                .transfer(&env.current_contract_address(), &admin, &balance);
            env.storage().persistent()
                .set(&DataKey::Balance(pwd.clone()), &0_i128);
        }
 
        // Remove expiry entry — grant window is fully closed
        env.storage().persistent().remove(&DataKey::Expiry(pwd.clone()));
 
        env.events().publish(
            (Symbol::new(&env, "clawback"), pwd),
            balance,
        );
    }
 
    // ── View functions ────────────────────────────────────────────────────────
 
    /// Returns the restricted token balance for a PWD.
    pub fn balance(env: Env, pwd: Address) -> i128 {
        env.storage().persistent()
            .get(&DataKey::Balance(pwd))
            .unwrap_or(0)
    }
 
    /// Returns true if the address is a registered PWD beneficiary.
    pub fn is_beneficiary(env: Env, address: Address) -> bool {
        env.storage().persistent()
            .get::<DataKey, bool>(&DataKey::Beneficiary(address))
            .unwrap_or(false)
    }
 
    /// Returns true if the address is a whitelisted merchant.
    pub fn is_merchant(env: Env, address: Address) -> bool {
        env.storage().persistent()
            .get::<DataKey, bool>(&DataKey::Merchant(address))
            .unwrap_or(false)
    }
 
    // ── Internal helpers ──────────────────────────────────────────────────────
 
    fn require_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
    }
 
    fn assert_beneficiary(env: &Env, addr: &Address) {
        let registered: bool = env.storage().persistent()
            .get::<DataKey, bool>(&DataKey::Beneficiary(addr.clone()))
            .unwrap_or(false);
        if !registered {
            panic_with_error!(env, Error::NotRegistered);
        }
    }
 
    pub fn admin(env: &Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }
 
    pub fn token(env: &Env) -> Address {
        env.storage().instance().get(&DataKey::Token).unwrap()
    }
}
 