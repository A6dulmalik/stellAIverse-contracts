#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Symbol, Address, Env, Vec};

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct StrategyConfig {
    pub id: Symbol,
    pub target_weight_bps: u32, // Basis points (e.g. 5000 = 50%)
    pub allocated_amount: i128,
    pub apy_bps: u32,           // Historical APY in basis points
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    FeeRecipient,
    PerfFeeBps,
    TotalDeposits,
    UserBalance(Address),
    Strategy(Symbol),
    StrategyList,
}

#[contract]
pub struct YieldAggregatorContract;

#[contractimpl]
impl YieldAggregatorContract {
    pub fn initialize(env: Env, admin: Address, fee_recipient: Address, perf_fee_bps: u32) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::FeeRecipient, &fee_recipient);
        env.storage().instance().set(&DataKey::PerfFeeBps, &perf_fee_bps);
        env.storage().instance().set(&DataKey::TotalDeposits, &0i128);
        
        let empty_list: Vec<Symbol> = Vec::new(&env);
        env.storage().instance().set(&DataKey::StrategyList, &empty_list);
    }

    pub fn deposit(env: Env, from: Address, _token: Address, amount: i128) {
        from.require_auth();
        if amount <= 0 {
            panic!("amount must be positive");
        }

        let user_key = DataKey::UserBalance(from.clone());
        let current_bal: i128 = env.storage().persistent().get(&user_key).unwrap_or(0);
        env.storage().persistent().set(&user_key, &(current_bal + amount));

        let total: i128 = env.storage().instance().get(&DataKey::TotalDeposits).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalDeposits, &(total + amount));
    }

    pub fn withdraw(env: Env, to: Address, _token: Address, amount: i128) {
        to.require_auth();
        if amount <= 0 {
            panic!("amount must be positive");
        }

        let user_key = DataKey::UserBalance(to.clone());
        let current_bal: i128 = env.storage().persistent().get(&user_key).unwrap_or(0);
        if current_bal < amount {
            panic!("insufficient user balance");
        }
        env.storage().persistent().set(&user_key, &(current_bal - amount));

        let total: i128 = env.storage().instance().get(&DataKey::TotalDeposits).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalDeposits, &(total - amount));
    }

    pub fn add_strategy(env: Env, strategy_id: Symbol, target_weight_bps: u32) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let strategy = StrategyConfig {
            id: strategy_id.clone(),
            target_weight_bps,
            allocated_amount: 0,
            apy_bps: 500, // 5% default APY
        };

        env.storage().instance().set(&DataKey::Strategy(strategy_id.clone()), &strategy);

        let mut list: Vec<Symbol> = env.storage().instance().get(&DataKey::StrategyList).unwrap();
        list.push_back(strategy_id);
        env.storage().instance().set(&DataKey::StrategyList, &list);
    }

    pub fn rebalance(env: Env) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let total: i128 = env.storage().instance().get(&DataKey::TotalDeposits).unwrap_or(0);
        let list: Vec<Symbol> = env.storage().instance().get(&DataKey::StrategyList).unwrap();

        for i in 0..list.len() {
            let id = list.get(i).unwrap();
            let mut strat: StrategyConfig = env.storage().instance().get(&DataKey::Strategy(id.clone())).unwrap();
            let target_amount = (total * (strat.target_weight_bps as i128)) / 10000;
            strat.allocated_amount = target_amount;
            env.storage().instance().set(&DataKey::Strategy(id), &strat);
        }
    }

    pub fn auto_compound(env: Env) {
        let total: i128 = env.storage().instance().get(&DataKey::TotalDeposits).unwrap_or(0);
        // Simulate 1% compound yield addition
        let yield_earned = total / 100;
        let perf_fee_bps: u32 = env.storage().instance().get(&DataKey::PerfFeeBps).unwrap_or(0);
        let fee = (yield_earned * (perf_fee_bps as i128)) / 10000;
        let net_yield = yield_earned - fee;

        env.storage().instance().set(&DataKey::TotalDeposits, &(total + net_yield));
    }

    pub fn emergency_withdraw(env: Env, to: Address, _token: Address) {
        to.require_auth();
        let user_key = DataKey::UserBalance(to.clone());
        let current_bal: i128 = env.storage().persistent().get(&user_key).unwrap_or(0);
        if current_bal > 0 {
            env.storage().persistent().set(&user_key, &0i128);
            let total: i128 = env.storage().instance().get(&DataKey::TotalDeposits).unwrap_or(0);
            let new_total = if total >= current_bal { total - current_bal } else { 0 };
            env.storage().instance().set(&DataKey::TotalDeposits, &new_total);
        }
    }

    pub fn get_strategy_allocation(env: Env, strategy_id: Symbol) -> u32 {
        let strat: StrategyConfig = env.storage().instance().get(&DataKey::Strategy(strategy_id)).unwrap();
        strat.target_weight_bps
    }

    pub fn get_user_balance(env: Env, user: Address) -> i128 {
        let user_key = DataKey::UserBalance(user);
        env.storage().persistent().get(&user_key).unwrap_or(0)
    }

    pub fn get_total_deposits(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::TotalDeposits).unwrap_or(0)
    }
}

#[cfg(test)]
mod test;
