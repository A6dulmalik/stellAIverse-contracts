use soroban_sdk::{Env, Symbol, String, Address};

// Core storage keys
pub const ADMIN: &str = "admin";
pub const VOTING_TOKEN: &str = "voting_token";
pub const PROPOSAL_COUNT: &str = "prop_count";
pub const VOTING_DELAY: &str = "voting_delay";
pub const VOTING_PERIOD: &str = "voting_period";
pub const TIMELOCK_DELAY: &str = "timelock_delay";
pub const QUORUM: &str = "quorum";
pub const APPROVAL_THRESHOLD: &str = "threshold";
pub const PROPOSAL_BASE: &str = "prop_";
pub const VOTE_BASE: &str = "vote_";
pub const DELEGATION_BASE: &str = "del_";

// Get proposal storage key
pub fn get_proposal_key(env: &Env, proposal_id: u64) -> Symbol {
    Symbol::new(env, &format!("{}{}", PROPOSAL_BASE, proposal_id))
}

// Get vote storage key for a specific voter and proposal
pub fn get_vote_key(env: &Env, proposal_id: u64, voter: &Address) -> Symbol {
    let voter_str = voter.to_string();
    Symbol::new(env, &format!("{}{}_{}", VOTE_BASE, proposal_id, voter_str))
}

// Get delegation storage key for a delegator
pub fn get_delegation_key(env: &Env, delegator: &Address) -> Symbol {
    let delegator_str = delegator.to_string();
    Symbol::new(env, &format!("{}{}", DELEGATION_BASE, delegator_str))
}