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
//  INITIALIZATION & ADMIN
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

// ═══════════════════════════════════════════════════════════════
//  TIER MANAGEMENT
// ═══════════════════════════════════════════════════════════════

#[test]
fn add_tier_successfully() {
    let (env, staking, _token, admin, _user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);
    assert_eq!(tier_id, 1);
    let tier = staking.get_tier(&tier_id);
    assert_eq!(tier.min_stake_amount, 1000);
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
fn deactivate_tier() {
    let (env, staking, _token, admin, _user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);
    staking.deactivate_tier(&admin, &tier_id);
    assert!(!staking.get_tier(&tier_id).active);
}

// ═══════════════════════════════════════════════════════════════
//  STAKING & UNSTAKING
// ═══════════════════════════════════════════════════════════════

#[test]
fn stake_tokens_successfully() {
    let (env, staking, _token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    let result = staking.stake(&user, &5000, &tier_id);
    assert_eq!(result.stake_id, 1);
    assert_eq!(staking.get_total_staked(), 5000);
}

#[test]
fn unstake_after_lock_period() {
    let (env, staking, _token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    env.ledger().set_timestamp(1000);
    staking.stake(&user, &5000, &tier_id);

    env.ledger().set_timestamp(87401);
    let result = staking.unstake(&user, &1);
    assert_eq!(result.penalty_amount, 0);
    assert_eq!(result.principal_returned, 5000);
    assert_eq!(staking.get_total_staked(), 0);
}

#[test]
fn unstake_early_with_penalty() {
    let (env, staking, _token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    env.ledger().set_timestamp(1000);
    staking.stake(&user, &10000, &tier_id);

    env.ledger().set_timestamp(4601);
    let result = staking.unstake(&user, &1);
    assert_eq!(result.penalty_amount, 500);
    assert_eq!(result.principal_returned, 9500);
}

#[test]
#[should_panic(expected = "Only staker can unstake")]
fn non_staker_cannot_unstake() {
    let (env, staking, _token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    env.ledger().set_timestamp(1000);
    staking.stake(&user, &5000, &tier_id);

    let stranger = Address::generate(&env);
    staking.unstake(&stranger, &1);
}

// ═══════════════════════════════════════════════════════════════
//  REWARD DISTRIBUTION & CLAIMS
// ═══════════════════════════════════════════════════════════════

#[test]
fn claim_rewards_after_staking() {
    let (env, staking, token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    env.ledger().set_timestamp(1000);
    staking.stake(&user, &10000, &tier_id);

    env.ledger().set_timestamp(1010);
    let claimed = staking.claim_rewards(&user, &1);
    assert!(claimed > 0);
    assert_eq!(token.balance(&user), 10_000_000 - 10000 + claimed);
}

#[test]
#[should_panic(expected = "No rewards to claim")]
fn cannot_claim_zero_rewards() {
    let (env, staking, _token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    env.ledger().set_timestamp(1000);
    staking.stake(&user, &10000, &tier_id);
    staking.claim_rewards(&user, &1);
}

#[test]
fn claim_rewards_batch() {
    let (env, staking, token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    env.ledger().set_timestamp(1000);
    staking.stake(&user, &5000, &tier_id);
    staking.stake(&user, &5000, &tier_id);

    env.ledger().set_timestamp(1010);
    let stake_ids = Vec::from_array(&env, [1, 2]);
    let total = staking.claim_rewards_batch(&user, &stake_ids);
    assert!(total > 0);
    assert_eq!(token.balance(&user), 10_000_000 - 10000 + total);
}

#[test]
fn reward_calculation_with_multiplier() {
    let (env, staking, token, admin, _user) = setup();
    let standard_tier = add_default_tier(&env, &staking, &admin);
    let premium_tier = add_premium_tier(&env, &staking, &admin);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    token.mint(&user_a, &1_000_000);
    token.mint(&user_b, &1_000_000);

    env.ledger().set_timestamp(1000);
    staking.stake(&user_a, &10000, &standard_tier);
    staking.stake(&user_b, &10000, &premium_tier);

    env.ledger().set_timestamp(1010);
    let claimed_a = staking.claim_rewards(&user_a, &1);
    let claimed_b = staking.claim_rewards(&user_b, &2);

    assert!(claimed_b > claimed_a);
}

#[test]
fn multiple_stakers_share_rewards_fairly() {
    let (env, staking, token, admin, _user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    token.mint(&user_a, &1_000_000);
    token.mint(&user_b, &1_000_000);

    env.ledger().set_timestamp(1000);
    staking.stake(&user_a, &10000, &tier_id);
    staking.stake(&user_b, &10000, &tier_id);

    env.ledger().set_timestamp(1010);
    let claimed_a = staking.claim_rewards(&user_a, &1);
    let claimed_b = staking.claim_rewards(&user_b, &2);
    assert_eq!(claimed_a, claimed_b);
}

// ═══════════════════════════════════════════════════════════════
//  EMERGENCY WITHDRAWAL
// ═══════════════════════════════════════════════════════════════

#[test]
fn emergency_withdraw_returns_principal_only() {
    let (env, staking, token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    env.ledger().set_timestamp(1000);
    staking.stake(&user, &10000, &tier_id);

    let result = staking.emergency_withdraw(&admin, &user, &1);
    assert_eq!(result.principal_returned, 10000);
    assert_eq!(result.rewards_claimed, 0);
    assert_eq!(staking.get_total_staked(), 0);
    assert_eq!(token.balance(&user), 10_000_000);
}

#[test]
fn emergency_withdraw_all() {
    let (env, staking, token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    env.ledger().set_timestamp(1000);
    staking.stake(&user, &5000, &tier_id);
    staking.stake(&user, &3000, &tier_id);

    let total = staking.emergency_withdraw_all(&admin, &user);
    assert_eq!(total, 8000);
    assert_eq!(staking.get_total_staked(), 0);
    assert_eq!(token.balance(&user), 10_000_000);
}

#[test]
#[should_panic(expected = "Unauthorized: caller is not admin")]
fn non_admin_cannot_emergency_withdraw() {
    let (env, staking, _token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    env.ledger().set_timestamp(1000);
    staking.stake(&user, &5000, &tier_id);

    let stranger = Address::generate(&env);
    staking.emergency_withdraw(&stranger, &user, &1);
}

// ═══════════════════════════════════════════════════════════════
//  FUND REWARDS
// ═══════════════════════════════════════════════════════════════

#[test]
fn fund_rewards() {
    let (env, staking, token, _admin, _user) = setup();
    let funder = Address::generate(&env);
    token.mint(&funder, &1_000_000);

    let contract_balance_before = token.balance(&staking.address);
    staking.fund_rewards(&funder, &50_000);

    assert_eq!(token.balance(&staking.address), contract_balance_before + 50_000);
}

#[test]
#[should_panic(expected = "Amount must be positive")]
fn fund_rewards_rejects_zero() {
    let (_env, staking, _token, _admin, _user) = setup();
    let funder = Address::generate(&_env);
    staking.fund_rewards(&funder, &0);
}

// ═══════════════════════════════════════════════════════════════
//  VIEW FUNCTIONS
// ═══════════════════════════════════════════════════════════════

#[test]
fn get_staking_info() {
    let (env, staking, _token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);
    staking.stake(&user, &5000, &tier_id);

    let info = staking.get_staking_info();
    assert_eq!(info.admin, admin);
    assert_eq!(info.total_staked, 5000);
    assert_eq!(info.tier_count, 1);
}

#[test]
fn get_last_reward_time_updates() {
    let (env, staking, _token, admin, user) = setup();
    let tier_id = add_default_tier(&env, &staking, &admin);

    assert_eq!(staking.get_last_reward_time(), 1000);

    env.ledger().set_timestamp(2000);
    staking.stake(&user, &5000, &tier_id);
    assert_eq!(staking.get_last_reward_time(), 2000);
}

#[test]
fn early_unstake_applies_correct_penalty() {
    let (env, staking, _token, admin, user) = setup();
    let standard_tier = add_default_tier(&env, &staking, &admin);
    let premium_tier = add_premium_tier(&env, &staking, &admin);

    env.ledger().set_timestamp(1000);
    staking.stake(&user, &10000, &standard_tier);

    env.ledger().set_timestamp(4601);
    let result_standard = staking.unstake(&user, &1);
    assert_eq!(result_standard.penalty_amount, 500);

    staking.stake(&user, &10000, &premium_tier);

    env.ledger().set_timestamp(4602);
    let result_premium = staking.unstake(&user, &2);
    assert_eq!(result_premium.penalty_amount, 1000);
}

#[test]
fn zero_penalty_tier() {
    let (env, staking, _token, admin, user) = setup();
    let name = Symbol::new(&env, "nopenalty");
    let tier_id = staking.add_tier(&admin, &name, &1000i128, &86400u64, &10000u32, &0u32);

    env.ledger().set_timestamp(1000);
    staking.stake(&user, &10000, &tier_id);

    env.ledger().set_timestamp(4601);
    let result = staking.unstake(&user, &1);
    assert_eq!(result.penalty_amount, 0);
}
