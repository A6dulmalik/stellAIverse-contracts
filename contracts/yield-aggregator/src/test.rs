#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

#[test]
fn test_yield_aggregator_workflow() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);
    let user = Address::generate(&env);
    let token = Address::generate(&env);

    let contract_id = env.register(YieldAggregatorContract, ());
    let client = YieldAggregatorContractClient::new(&env, &contract_id);

    client.initialize(&admin, &fee_recipient, &1000); // 10% performance fee

    let strat_a = Symbol::new(&env, "LENDING");
    let strat_b = Symbol::new(&env, "AMM");

    client.add_strategy(&strat_a, &6000); // 60%
    client.add_strategy(&strat_b, &4000); // 40%

    client.deposit(&user, &token, &10000);
    assert_eq!(client.get_user_balance(&user), 10000);
    assert_eq!(client.get_total_deposits(), 10000);

    client.rebalance();

    client.auto_compound();
    assert!(client.get_total_deposits() > 10000);

    client.withdraw(&user, &token, &5000);
    assert_eq!(client.get_user_balance(&user), 5000);

    client.emergency_withdraw(&user, &token);
    assert_eq!(client.get_user_balance(&user), 0);
}
