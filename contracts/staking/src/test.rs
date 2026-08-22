use super::*;
use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

#[contract]
pub struct MockToken;

#[derive(Clone)]
#[contracttype]
pub enum MockTokenKey {
    Balance(Address),
}

#[contractimpl]
impl MockToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        if amount <= 0 {
            panic!("Mint amount must be positive");
        }
        let key = MockTokenKey::Balance(to);
        let current: i128 = env.storage().instance().get(&key).unwrap_or(0);
        env.storage()
            .instance()
            .set(&key, &(current.checked_add(amount).unwrap()));
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage()
            .instance()
            .get(&MockTokenKey::Balance(id))
            .unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        if amount <= 0 {
            panic!("Transfer amount must be positive");
        }

        let from_key = MockTokenKey::Balance(from.clone());
        let from_balance: i128 = env.storage().instance().get(&from_key).unwrap_or(0);
        if from_balance < amount {
            panic!("Insufficient balance");
        }
        env.storage()
            .instance()
            .set(&from_key, &(from_balance - amount));

        let to_key = MockTokenKey::Balance(to);
        let to_balance: i128 = env.storage().instance().get(&to_key).unwrap_or(0);
        env.storage()
            .instance()
            .set(&to_key, &(to_balance.checked_add(amount).unwrap()));
    }
}

fn setup() -> (
    Env,
    StakingClient<'static>,
    MockTokenClient<'static>,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let staking_id = env.register(Staking, ());
    let staking = StakingClient::new(&env, &staking_id);

    let token_id = env.register(MockToken, ());
    let token = MockTokenClient::new(&env, &token_id);
    token.mint(&user, &10_000_000);
    token.mint(&staking_id, &10_000_000);

    staking.initialize(&admin, &token_id, &100i128);

    (env, staking, token, admin, user)
}

#[test]
fn initializes_correctly() {
    let (_env, staking, _token, admin, _user) = setup();

    assert_eq!(staking.get_admin(), admin);
    assert_eq!(staking.get_reward_rate(), 100);
    assert_eq!(staking.get_total_staked(), 0);
    assert_eq!(staking.get_total_rewards_distributed(), 0);
    assert!(!staking.is_paused());
}

#[test]
#[should_panic(expected = "Already initialized")]
fn cannot_initialize_twice() {
    let (env, staking, _token, admin, _user) = setup();
    let token_id = env.register(MockToken, ());
    staking.initialize(&admin, &token_id, &100i128);
}

#[test]
fn pause_and_unpause() {
    let (_env, staking, _token, admin, _user) = setup();

    staking.pause(&admin);
    assert!(staking.is_paused());

    staking.unpause(&admin);
    assert!(!staking.is_paused());
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not admin")]
fn non_admin_cannot_pause() {
    let (_env, staking, _token, _admin, user) = setup();
    staking.pause(&user);
}
