#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Env},
        Address, String, Vec, Symbol,
    };
    use super::{GovernanceContract, GovernanceContractClient, VoteType, ProposalState};

    #[test]
    fn test_initialize_contract() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let token = Address::generate(&env);

        let governance_id = env.register_contract(None, GovernanceContract);
        let client = GovernanceContractClient::new(&env, &governance_id);

        // Initialize with default parameters
        client.initialize(
            &admin,
            &token,
            &1,      // voting_delay (1 block)
            &100,    // voting_period (100 blocks)
            &86400,  // timelock_delay (1 day)
            &2000,   // quorum (20%)
            &5100,   // approval_threshold (51%)
            &1000,   // proposal_threshold (minimum tokens to propose)
        );

        // Verify we can get proposal count (should be 0)
        let ids = client.get_proposal_ids();
        assert_eq!(ids.len(), 0);
    }

    #[test]
    #[should_panic(expected = "1")] // AlreadyInitialized
    fn test_cannot_double_initialize() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let token = Address::generate(&env);

        let governance_id = env.register_contract(None, GovernanceContract);
        let client = GovernanceContractClient::new(&env, &governance_id);

        client.initialize(
            &admin, &token, &1, &100, &86400, &2000, &5100, &1000
        );
        
        // Try to initialize again - should panic
        client.initialize(
            &admin, &token, &1, &100, &86400, &2000, &5100, &1000
        );
    }

    #[test]
    fn test_create_proposal() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let proposer = Address::generate(&env);

        let governance_id = env.register_contract(None, GovernanceContract);
        let client = GovernanceContractClient::new(&env, &governance_id);

        client.initialize(
            &admin,
            &token,
            &1,
            &100,
            &86400,
            &2000,
            &5100,
            &100, // Lower threshold for testing
        );

        // Mock token balance for proposer
        // In a real test, we would mock the token contract's balance_of response

        let description = String::from_str(&env, "Test proposal to update parameters");
        let targets = Vec::new(&env);
        let values = Vec::new(&env);
        let functions = Vec::new(&env);
        let calldatas = Vec::new(&env);

        // Note: This would normally fail if balance check fails, but in test we demonstrate the flow
        // The actual token balance check would work with a real token contract
        let proposal_id = client.propose(
            &proposer,
            &description,
            &targets,
            &values,
            &functions,
            &calldatas,
        );

        let ids = client.get_proposal_ids();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids.get(0).unwrap(), proposal_id);
    }

    #[test]
    fn test_vote_delegation() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let delegator = Address::generate(&env);
        let delegatee = Address::generate(&env);

        let governance_id = env.register_contract(None, GovernanceContract);
        let client = GovernanceContractClient::new(&env, &governance_id);

        client.initialize(
            &admin, &token, &1, &100, &86400, &2000, &5100, &1000
        );

        // Delegate voting power
        client.delegate(&delegator, &delegatee, &1000);
    }

    #[test]
    #[should_panic(expected = "16")] // DelegationToSelf
    fn test_cannot_delegate_to_self() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let user = Address::generate(&env);

        let governance_id = env.register_contract(None, GovernanceContract);
        let client = GovernanceContractClient::new(&env, &governance_id);

        client.initialize(
            &admin, &token, &1, &100, &86400, &2000, &5100, &1000
        );

        // Try to delegate to self - should panic
        client.delegate(&user, &user, &1000);
    }

    #[test]
    fn test_update_parameters() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let token = Address::generate(&env);

        let governance_id = env.register_contract(None, GovernanceContract);
        let client = GovernanceContractClient::new(&env, &governance_id);

        client.initialize(
            &admin, &token, &1, &100, &86400, &2000, &5100, &1000
        );

        // Update only some parameters
        client.set_voting_params(
            &admin,
            None,           // don't update voting_delay
            Some(200),      // update voting_period
            None,           // don't update timelock
            Some(2500),     // update quorum to 25%
            None,           // don't update threshold
        );
    }
}