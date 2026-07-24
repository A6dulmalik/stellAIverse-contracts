use soroban_sdk::{contracttype, Address, String, Vec, Map};
use crate::governance::VoteType;

// Proposal States
#[derive(Clone, Copy, PartialEq, Debug)]
#[contracttype]
pub enum ProposalState {
    Pending = 0,    // Waiting for voting delay to pass
    Active = 1,     // Voting is active
    Canceled = 2,   // Proposal was canceled
    Defeated = 3,   // Proposal failed to pass
    Succeeded = 4,  // Proposal passed but not queued
    Queued = 5,     // Queued for timelock execution
    Expired = 6,    // Timelock expired, not executed
    Executed = 7,   // Successfully executed
}

// Vote types
#[derive(Clone, Copy, PartialEq, Debug)]
#[contracttype]
pub enum VoteType {
    Against = 0,
    For = 1,
    Abstain = 2,
}

// Timelocked operation
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct TimelockedCall {
    pub target: Address,
    pub value: i128,
    pub function: String,
    pub data: Vec<u8>,
    pub eta: u64,
    pub executed: bool,
}

// Proposal structure
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub description: String,
    pub targets: Vec<Address>,
    pub values: Vec<i128>,
    pub functions: Vec<String>,
    pub calldatas: Vec<Vec<u8>>,
    pub vote_start: u64,
    pub vote_end: u64,
    pub eta: u64,
    pub for_votes: i128,
    pub against_votes: i128,
    pub abstain_votes: i128,
    pub canceled: bool,
    pub executed: bool,
    pub created_at: u64,
}

// Proposal vote tracker
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct ProposalVote {
    pub has_voted: bool,
    pub vote_type: VoteType,
    pub weight: i128,
}

// Delegation information
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct Delegation {
    pub delegator: Address,
    pub delegatee: Address,
    pub amount: i128,
    pub timestamp: u64,
}

// Governance settings
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct GovernanceSettings {
    pub voting_delay: u64,      // Blocks before voting starts
    pub voting_period: u64,     // Blocks voting lasts
    pub timelock_delay: u64,    // Seconds to wait before execution
    pub quorum: i128,           // Minimum percentage of supply needed (in basis points)
    pub approval_threshold: i128,// Percentage of votes needed to pass (basis points)
    pub proposal_threshold: i128,// Minimum tokens required to create a proposal
}