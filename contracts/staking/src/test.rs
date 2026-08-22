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

fn add_default_tier(
    env: &Env,
    staking: &StakingClient<'_>,
    admin: &Address,
) -> u32 {
    let tier_name = Symbol::new(env, "standard");
    staking.add_tier(
        admin,
        &tier_name,
        &1000i128,
        &86400u64,
        &10000u32,
        &500u32,
    )
}

fn add_premium_tier(
    env: &Env,
    staking: &StakingClient<'_>,
    admin: &Address,
) -> u32 {
    let tier_name = Symbol::new(env, "premium");
    staking.add_tier(
        admin,
        &tier_name,
        &10000i128,
        &604800u64,
        &15000u32,
        &1000u32,
    )
}

// ═══════════════════════════════════════════════════════════════
//  INITIALIZATION & ADMIN TESTS
// ═══════════════════════════════════════════════════════════════

#[test]
fn initializes_correctly() {
    let (_env, staking, _token, admin, _user) = setup();
    assert_eq!(staking.get_admin(), admin);
    assert_eq!(staking.get_reward_rate(), 100);
    assert_eq!(staking.get_total_staked(), 0);
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

#[test]
fn set_reward_rate() {
    let (_env, staking, _token, admin, _user) = setup();
    staking.set_reward_rate(&admin, &200);
    assert_eq!(staking.get_reward_rate(), 200);
}

#[test]
#[should_panic(expected = "Reward rate cannot be negative")]
fn set_reward_rate_rejects_negative() {
    let (_env, staking, _token, admin, _user) = setup();
    staking.set_reward_rate(&admin, &-1);
}

// ═══════════════════════════════════════════════════════════════
//  TIER MANAGEMENT TESTS
// ═══════════════════════════════════════════════════════════════

#[test]
fn add_tier_successfully() {
    let (env, staking, _token, admin, _user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    assert_eq!(tier_id, 1);
    let tier = staking.get_tier(&tier_id);
    assert_eq!(tier.tier_id, 1);
    assert_eq!(tier.min_stake_amount, 1000);
    assert_eq!(tier.lock_duration_seconds, 86400);
    assert_eq!(tier.reward_multiplier_bps, 10000);
    assert_eq!(tier.penalty_bps, 500);
    assert!(tier.active);
}

#[test]
fn add_multiple_tiers() {
    let (env, staking, _token, admin, _user) = setup();
    let tier1 = add_default_tier(&env, &staking, &admin);
    let tier2 = add_premium_tier(&env, &staking, &admin);
    assert_eq!(tier1, 1);
    assert_eq!(tier2, 2);
    assert_eq!(staking.get_tier_ids().len(), 2);
}

#[test]
fn update_tier() {
    let (env, staking, _token, admin, _user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);
    let new_name = Symbol::new(&env, "updated");
    let updated = staking.update_tier(
        &admin,
        &tier_id,
        &Some(new_name),
        &Some(2000),
        &Some(172800),
        &Some(20000),
        &Some(1000),
    );
    assert_eq!(updated.min_stake_amount, 2000);
    assert_eq!(updated.reward_multiplier_bps, 20000);
}

#[test]
fn deactivate_tier() {
    let (env, staking, _token, admin, _user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);
    staking.deactivate_tier(&admin, &tier_id);
    let tier = staking.get_tier(&tier_id);
    assert!(!tier.active);
}

#[test]
#[should_panic(expected = "Tier already inactive")]
fn cannot_deactivate_twice() {
    let (env, staking, _token, admin, _user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);
    staking.deactivate_tier(&admin, &tier_id);
    staking.deactivate_tier(&admin, &tier_id);
}

#[test]
#[should_panic(expected = "Minimum stake amount must be positive")]
fn add_tier_rejects_zero_min_amount() {
    let (env, staking, _token, admin, _user) = setup();
    let tier_name = Symbol::new(&env, "bad");
    staking.add_tier(&admin, &tier_name, &0i128, &86400u64, &10000u32, &500u32);
}

#[test]
#[should_panic(expected = "Lock duration must be positive")]
fn add_tier_rejects_zero_duration() {
    let (env, staking, _token, admin, _user) = setup();
    let tier_name = Symbol::new(&env, "bad");
    staking.add_tier(&admin, &tier_name, &1000i128, &0u64, &10000u32, &500u32);
}

#[test]
#[should_panic(expected = "Penalty exceeds 100%")]
fn add_tier_rejects_excessive_penalty() {
    let (env, staking, _token, admin, _user) = setup();
    let tier_name = Symbol::new(&env, "bad");
    staking.add_tier(&admin, &tier_name, &1000i128, &86400u64, &10000u32, &10001u32);
}

#[test]
#[should_panic(expected = "Tier is not active")]
fn update_inactive_tier_fails() {
    let (env, staking, _token, admin, _user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);
    staking.deactivate_tier(&admin, &tier_id);
    staking.update_tier(&admin, &tier_id, &None, &None, &None, &None, &None);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not admin")]
fn non_admin_cannot_add_tier() {
    let (env, staking, _token, _admin, user) = setup();
    let tier_name = Symbol::new(&env, "standard");
    staking.add_tier(&user, &tier_name, &1000i128, &86400u64, &10000u32, &500u32);
}

#[test]
fn tier_ids_tracking() {
    let (env, staking, _token, admin, _user) = setup();
    assert_eq!(staking.get_tier_ids().len(), 0);
    let _ = add_default_tier(&env, &staking, &admin);
    assert_eq!(staking.get_tier_ids().len(), 1);
    let _ = add_premium_tier(&env, &staking, &admin);
    assert_eq!(staking.get_tier_ids().len(), 2);
}
