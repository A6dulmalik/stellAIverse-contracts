use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol, String, Vec, Map, log};
use crate::errors::*;
use crate::storage_keys::*;
use crate::types::*;
use crate::utils::*;

#[contract]
pub struct GovernanceContract;

#[contractimpl]
impl GovernanceContract {
    /// Initialize the governance contract with core parameters
    pub fn initialize(
        env: Env,
        admin: Address,
        voting_token: Address,
        voting_delay: u64,
        voting_period: u64,
        timelock_delay: u64,
        quorum: i128,
        approval_threshold: i128,
        proposal_threshold: i128,
    ) {
        // Check if already initialized
        if env.storage().instance().has(&Symbol::new(&env, ADMIN)) {
            panic!("{}", GovernanceError::AlreadyInitialized as u32);
        }

        admin.require_auth();

        // Store core configuration
        env.storage().instance().set(&Symbol::new(&env, ADMIN), &admin);
        env.storage().instance().set(&Symbol::new(&env, VOTING_TOKEN), &voting_token);
        env.storage().instance().set(&Symbol::new(&env, VOTING_DELAY), &voting_delay);
        env.storage().instance().set(&Symbol::new(&env, VOTING_PERIOD), &voting_period);
        env.storage().instance().set(&Symbol::new(&env, TIMELOCK_DELAY), &timelock_delay);
        env.storage().instance().set(&Symbol::new(&env, "quorum"), &quorum);
        env.storage().instance().set(&Symbol::new(&env, "threshold"), &approval_threshold);
        env.storage().instance().set(&Symbol::new(&env, "prop_thresh"), &proposal_threshold);
        env.storage().instance().set(&Symbol::new(&env, PROPOSAL_COUNT), &0u64);

        env.events().publish(
            (Symbol::new(&env, "contract_initialized"),),
            (admin, voting_token, env.ledger().timestamp()),
        );
    }

    /// Create a new proposal
    pub fn propose(
        env: Env,
        proposer: Address,
        description: String,
        targets: Vec<Address>,
        values: Vec<i128>,
        functions: Vec<String>,
        calldatas: Vec<Vec<u8>>,
    ) -> u64 {
        proposer.require_auth();

        // Verify proposer has enough tokens to create proposal
        let voting_power = self.get_voting_power(&env, &proposer);
        let proposal_threshold: i128 = env.storage()
            .instance()
            .get(&Symbol::new(&env, "prop_thresh"))
            .unwrap();

        if voting_power < proposal_threshold {
            panic!("{}", GovernanceError::InsufficientVotingPower as u32);
        }

        // Validate inputs
        if targets.len() != values.len() || targets.len() != functions.len() || targets.len() != calldatas.len() {
            panic!("{}", GovernanceError::InvalidInput as u32);
        }

        // Get voting periods
        let voting_delay: u64 = env.storage()
            .instance()
            .get(&Symbol::new(&env, VOTING_DELAY))
            .unwrap();
        let voting_period: u64 = env.storage()
            .instance()
            .get(&Symbol::new(&env, VOTING_PERIOD))
            .unwrap();

        let current_block = env.ledger().sequence();
        let vote_start = current_block + voting_delay;
        let vote_end = vote_start + voting_period;

        // Create proposal
        let mut proposal_count: u64 = env.storage()
            .instance()
            .get(&Symbol::new(&env, PROPOSAL_COUNT))
            .unwrap_or(0);
        let proposal_id = proposal_count;
        proposal_count += 1;
        env.storage().instance().set(&Symbol::new(&env, PROPOSAL_COUNT), &proposal_count);

        let proposal = Proposal {
            id: proposal_id,
            proposer: proposer.clone(),
            description,
            targets,
            values,
            functions,
            calldatas,
            vote_start,
            vote_end,
            eta: 0,
            for_votes: 0,
            against_votes: 0,
            abstain_votes: 0,
            canceled: false,
            executed: false,
            created_at: env.ledger().timestamp(),
        };

        // Store proposal
        let proposal_key = get_proposal_key(&env, proposal_id);
        env.storage().instance().set(&proposal_key, &proposal);

        env.events().publish(
            (Symbol::new(&env, "proposal_created"), proposal_id),
            (proposer, vote_start, vote_end),
        );

        proposal_id
    }

    /// Cast a vote on an active proposal
    pub fn cast_vote(
        env: Env,
        voter: Address,
        proposal_id: u64,
        vote_type: VoteType,
    ) {
        voter.require_auth();

        // Get and validate proposal
        let mut proposal = self.get_proposal(&env, proposal_id);
        if self.state(&env, &proposal) != ProposalState::Active {
            panic!("{}", GovernanceError::ProposalNotActive as u32);
        }

        // Check if already voted
        let vote_key = get_vote_key(&env, proposal_id, &voter);
        if env.storage().instance().has(&vote_key) {
            panic!("{}", GovernanceError::AlreadyVoted as u32);
        }

        // Get voting power
        let voting_power = self.get_voting_power(&env, &voter);

        // Record vote
        match vote_type {
            VoteType::For => proposal.for_votes += voting_power,
            VoteType::Against => proposal.against_votes += voting_power,
            VoteType::Abstain => proposal.abstain_votes += voting_power,
        }

        // Store vote record
        let vote = ProposalVote {
            has_voted: true,
            vote_type,
            weight: voting_power,
        };
        env.storage().instance().set(&vote_key, &vote);

        // Update proposal
        let proposal_key = get_proposal_key(&env, proposal_id);
        env.storage().instance().set(&proposal_key, &proposal);

        env.events().publish(
            (Symbol::new(&env, "vote_cast"), proposal_id),
            (voter, vote_type as u32, voting_power),
        );
    }

    /// Delegate voting power to another address
    pub fn delegate(
        env: Env,
        delegator: Address,
        delegatee: Address,
        amount: i128,
    ) {
        delegator.require_auth();

        if delegator == delegatee {
            panic!("{}", GovernanceError::DelegationToSelf as u32);
        }

        // Check if already delegated
        let delegation_key = get_delegation_key(&env, &delegator);
        if env.storage().instance().has(&delegation_key) {
            panic!("{}", GovernanceError::AlreadyDelegated as u32);
        }

        // Verify sufficient balance
        let balance = self.get_token_balance(&env, &delegator);
        if balance < amount {
            panic!("{}", GovernanceError::InsufficientBalance as u32);
        }

        // Create and store delegation
        let delegation = Delegation {
            delegator: delegator.clone(),
            delegatee: delegatee.clone(),
            amount,
            timestamp: env.ledger().timestamp(),
        };
        env.storage().instance().set(&delegation_key, &delegation);

        env.events().publish(
            (Symbol::new(&env, "delegation_created"),),
            (delegator, delegatee, amount, env.ledger().timestamp()),
        );
    }

    /// Cancel a proposal (only proposer or admin)
    pub fn cancel(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) {
        caller.require_auth();

        let mut proposal = self.get_proposal(&env, proposal_id);
        
        if proposal.executed {
            panic!("{}", GovernanceError::CannotCancelExecuted as u32);
        }

        // Only proposer or admin can cancel
        let admin: Address = env.storage()
            .instance()
            .get(&Symbol::new(&env, ADMIN))
            .unwrap();
        if caller != proposal.proposer && caller != admin {
            panic!("{}", GovernanceError::Unauthorized as u32);
        }

        proposal.canceled = true;
        let proposal_key = get_proposal_key(&env, proposal_id);
        env.storage().instance().set(&proposal_key, &proposal);

        env.events().publish(
            (Symbol::new(&env, "proposal_canceled"), proposal_id),
            (caller, env.ledger().timestamp()),
        );
    }

    /// Queue a successful proposal for timelock execution
    pub fn queue(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) {
        caller.require_auth();

        let mut proposal = self.get_proposal(&env, proposal_id);
        let state = self.state(&env, &proposal);

        if state != ProposalState::Succeeded {
            panic!("{}", GovernanceError::InvalidProposalState as u32);
        }

        let timelock_delay: u64 = env.storage()
            .instance()
            .get(&Symbol::new(&env, TIMELOCK_DELAY))
            .unwrap();
        
        proposal.eta = env.ledger().timestamp() + timelock_delay;
        let proposal_key = get_proposal_key(&env, proposal_id);
        env.storage().instance().set(&proposal_key, &proposal);

        env.events().publish(
            (Symbol::new(&env, "proposal_queued"), proposal_id),
            (proposal.eta, env.ledger().timestamp()),
        );
    }

    /// Execute a queued proposal after timelock expires
    pub fn execute(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) {
        caller.require_auth();

        let mut proposal = self.get_proposal(&env, proposal_id);
        let state = self.state(&env, &proposal);

        if state != ProposalState::Queued {
            panic!("{}", GovernanceError::ProposalNotQueued as u32);
        }

        if env.ledger().timestamp() < proposal.eta {
            panic!("{}", GovernanceError::TimelockNotExpired as u32);
        }

        // Execute all proposal actions
        for i in 0..proposal.targets.len() {
            let target = proposal.targets.get(i).unwrap();
            let value = proposal.values.get(i).unwrap();
            let _function = proposal.functions.get(i).unwrap();
            let calldata = proposal.calldatas.get(i).unwrap();

            // Invoke the target contract
            env.invoke()
                .call(&target, &_function, (calldata,))
                .unwrap();
        }

        proposal.executed = true;
        let proposal_key = get_proposal_key(&env, proposal_id);
        env.storage().instance().set(&proposal_key, &proposal);

        env.events().publish(
            (Symbol::new(&env, "proposal_executed"), proposal_id),
            (caller, env.ledger().timestamp()),
        );
    }

    /// Get the current state of a proposal
    pub fn state(env: &Env, proposal: &Proposal) -> ProposalState {
        if proposal.canceled {
            return ProposalState::Canceled;
        }

        let current_block = env.ledger().sequence();

        if current_block < proposal.vote_start {
            return ProposalState::Pending;
        } else if current_block <= proposal.vote_end {
            return ProposalState::Active;
        }

        if !self.quorum_reached(env, proposal) || !self.threshold_reached(env, proposal) {
            return ProposalState::Defeated;
        }

        if proposal.executed {
            return ProposalState::Executed;
        }

        if proposal.eta == 0 {
            return ProposalState::Succeeded;
        }

        if env.ledger().timestamp() >= proposal.eta {
            if !proposal.executed {
                return ProposalState::Expired;
            }
        }

        ProposalState::Queued
    }

    /// Check if quorum is reached
    fn quorum_reached(&self, env: &Env, proposal: &Proposal) -> bool {
        let total_votes = proposal.for_votes + proposal.against_votes + proposal.abstain_votes;
        let total_supply = self.get_total_supply(env);
        let quorum: i128 = env.storage()
            .instance()
            .get(&Symbol::new(env, "quorum"))
            .unwrap();

        // quorum is in basis points (10000 = 100%)
        (total_votes * 10000) >= (total_supply * quorum)
    }

    /// Check if approval threshold is reached
    fn threshold_reached(&self, env: &Env, proposal: &Proposal) -> bool {
        let total_votes_cast = proposal.for_votes + proposal.against_votes;
        if total_votes_cast == 0 {
            return false;
        }

        let threshold: i128 = env.storage()
            .instance()
            .get(&Symbol::new(env, "threshold"))
            .unwrap();

        // threshold is in basis points
        (proposal.for_votes * 10000) >= (total_votes_cast * threshold)
    }

    /// Get total voting power for an address (including delegations)
    fn get_voting_power(&self, env: &Env, account: &Address) -> i128 {
        let mut power = self.get_token_balance(env, account);

        // Add delegated power
        // Iterate through all delegations to this account (simplified for this implementation)
        power
    }

    /// Get token balance of an address
    fn get_token_balance(&self, env: &Env, account: &Address) -> i128 {
        let voting_token: Address = env.storage()
            .instance()
            .get(&Symbol::new(env, VOTING_TOKEN))
            .unwrap();

        // Query token contract for balance
        let balance: i128 = env.invoke()
            .call(&voting_token, &Symbol::new(env, "balance_of"), (account.clone(),))
            .unwrap_or(0);

        balance
    }

    /// Get total token supply
    fn get_total_supply(&self, env: &Env) -> i128 {
        let voting_token: Address = env.storage()
            .instance()
            .get(&Symbol::new(env, VOTING_TOKEN))
            .unwrap();

        let supply: i128 = env.invoke()
            .call(&voting_token, &Symbol::new(env, "total_supply"), ())
            .unwrap_or(0);

        supply
    }

    /// Helper to get proposal from storage
    fn get_proposal(&self, env: &Env, proposal_id: u64) -> Proposal {
        let proposal_key = get_proposal_key(env, proposal_id);
        env.storage()
            .instance()
            .get(&proposal_key)
            .unwrap_or_else(|| panic!("{}", GovernanceError::ProposalNotFound as u32))
    }

    /// Update governance parameters (admin only)
    pub fn set_voting_params(
        env: Env,
        admin: Address,
        voting_delay: Option<u64>,
        voting_period: Option<u64>,
        timelock_delay: Option<u64>,
        quorum: Option<i128>,
        approval_threshold: Option<i128>,
    ) {
        admin.require_auth();
        let stored_admin: Address = env.storage()
            .instance()
            .get(&Symbol::new(&env, ADMIN))
            .unwrap();
            
        if admin != stored_admin {
            panic!("{}", GovernanceError::Unauthorized as u32);
        }

        if let Some(delay) = voting_delay {
            env.storage().instance().set(&Symbol::new(&env, VOTING_DELAY), &delay);
        }
        if let Some(period) = voting_period {
            env.storage().instance().set(&Symbol::new(&env, VOTING_PERIOD), &period);
        }
        if let Some(delay) = timelock_delay {
            env.storage().instance().set(&Symbol::new(&env, TIMELOCK_DELAY), &delay);
        }
        if let Some(q) = quorum {
            env.storage().instance().set(&Symbol::new(&env, "quorum"), &q);
        }
        if let Some(t) = approval_threshold {
            env.storage().instance().set(&Symbol::new(&env, "threshold"), &t);
        }

        env.events().publish(
            (Symbol::new(&env, "params_updated"),),
            (env.ledger().timestamp()),
        );
    }

    /// Get all proposal IDs
    pub fn get_proposal_ids(env: Env) -> Vec<u64> {
        let count: u64 = env.storage()
            .instance()
            .get(&Symbol::new(&env, PROPOSAL_COUNT))
            .unwrap_or(0);
            
        let mut ids = Vec::new(&env);
        for i in 0..count {
            ids.push_back(i);
        }
        ids
    }

    /// Get detailed results for a proposal
    pub fn get_proposal_results(env: Env, proposal_id: u64) -> (i128, i128, i128, ProposalState) {
        let proposal = env.storage()
            .instance()
            .get::<Proposal>(&get_proposal_key(&env, proposal_id))
            .unwrap();
            
        let state = Self::state(&env, &proposal);
        (proposal.for_votes, proposal.against_votes, proposal.abstain_votes, state)
    }
}