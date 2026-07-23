use soroban_sdk::{Env, Address, Symbol};

/// Helper function to safely add two numbers with overflow check
pub fn safe_add(a: i128, b: i128) -> i128 {
    a.checked_add(b).unwrap_or_else(|| panic!("Overflow in addition"))
}

/// Helper function to safely subtract two numbers with underflow check
pub fn safe_sub(a: i128, b: i128) -> i128 {
    a.checked_sub(b).unwrap_or_else(|| panic!("Underflow in subtraction"))
}

/// Calculate basis points percentage (amount * bps / 10000)
pub fn calculate_bps(amount: i128, bps: i128) -> i128 {
    (amount * bps) / 10000
}

/// Verify that all targets in a proposal are whitelisted (for safety)
pub fn verify_targets(env: &Env, targets: &[Address], whitelist: &[Address]) -> bool {
    for target in targets {
        if !whitelist.contains(target) {
            return false;
        }
    }
    true
}

/// Get current block timestamp
pub fn get_timestamp(env: &Env) -> u64 {
    env.ledger().timestamp()
}

/// Get current block number
pub fn get_block_number(env: &Env) -> u32 {
    env.ledger().sequence()
}