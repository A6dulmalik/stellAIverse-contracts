use soroban_sdk::{Symbol, Env};

// Instance storage keys
pub const ADMIN: &str = "admin";
pub const PROVIDER_COUNT: &str = "prov_cnt";
pub const FEED_COUNT: &str = "feed_cnt";
pub const TREASURY: &str = "treasury";
pub const BASE_REWARD_RATE: &str = "base_reward";

// Prefixes for map storage
pub const PROVIDER_PREFIX: &str = "prov_";      // provider_<address>
pub const PRICE_FEED_PREFIX: &str = "feed_";    // feed_<feed_id>
pub const LATEST_PRICE_PREFIX: &str = "lp_";    // lp_<feed_id>
pub const PRICE_HISTORY_PREFIX: &str = "ph_";   // ph_<feed_id>
pub const CB_STATE_PREFIX: &str = "cb_";        // cb_<feed_id>
pub const FALLBACK_CFG_PREFIX: &str = "fb_";    // fb_<feed_id>
pub const PROVIDER_HEALTH_PREFIX: &str = "phc_"; // phc_<provider>
pub const CUSTOM_FEED_PREFIX: &str = "cf_";      // cf_<feed_id>
pub const CUSTOM_DATA_PREFIX: &str = "cd_";      // cd_<feed_id>
pub const SUBSCRIPTION_PREFIX: &str = "sub_";    // sub_<user>_<feed>
pub const RATE_LIMIT_PREFIX: &str = "rl_";       // rl_<user>
pub const INCENTIVE_BALANCE_PREFIX: &str = "ib_";// ib_<provider>

// Helper to create provider storage key
pub fn get_provider_key(env: &Env, provider: &Address) -> Symbol {
    let key_str = format!("{}{}", PROVIDER_PREFIX, provider.to_string());
    Symbol::new(env, &key_str)
}

// Helper to create price feed key
pub fn get_feed_key(env: &Env, feed_id: &Symbol) -> Symbol {
    let key_str = format!("{}{}", PRICE_FEED_PREFIX, feed_id.to_string());
    Symbol::new(env, &key_str)
}

// Helper to get latest price key
pub fn get_latest_price_key(env: &Env, feed_id: &Symbol) -> Symbol {
    let key_str = format!("{}{}", LATEST_PRICE_PREFIX, feed_id.to_string());
    Symbol::new(env, &key_str)
}

// Helper to get price history key
pub fn get_price_history_key(env: &Env, feed_id: &Symbol) -> Symbol {
    let key_str = format!("{}{}", PRICE_HISTORY_PREFIX, feed_id.to_string());
    Symbol::new(env, &key_str)
}

// Helper to get circuit breaker state key
pub fn get_circuit_breaker_key(env: &Env, feed_id: &Symbol) -> Symbol {
    let key_str = format!("{}{}", CB_STATE_PREFIX, feed_id.to_string());
    Symbol::new(env, &key_str)
}

// Helper to get fallback config key
pub fn get_fallback_config_key(env: &Env, feed_id: &Symbol) -> Symbol {
    let key_str = format!("{}{}", FALLBACK_CFG_PREFIX, feed_id.to_string());
    Symbol::new(env, &key_str)
}

// Helper to get provider health key
pub fn get_provider_health_key(env: &Env, provider: &Address) -> Symbol {
    let key_str = format!("{}{}", PROVIDER_HEALTH_PREFIX, provider.to_string());
    Symbol::new(env, &key_str)
}

// Helper to get custom feed key
pub fn get_custom_feed_key(env: &Env, feed_id: &Symbol) -> Symbol {
    let key_str = format!("{}{}", CUSTOM_FEED_PREFIX, feed_id.to_string());
    Symbol::new(env, &key_str)
}

// Helper to get custom data key
pub fn get_custom_data_key(env: &Env, feed_id: &Symbol) -> Symbol {
    let key_str = format!("{}{}", CUSTOM_DATA_PREFIX, feed_id.to_string());
    Symbol::new(env, &key_str)
}

// Helper to get subscription key
pub fn get_subscription_key(env: &Env, subscriber: &Address, feed_id: &Symbol) -> Symbol {
    let key_str = format!("sub_{}_{}", subscriber.to_string(), feed_id.to_string());
    Symbol::new(env, &key_str)
}

// Helper to get rate limit key
pub fn get_rate_limit_key(env: &Env, user: &Address) -> Symbol {
    let key_str = format!("{}{}", RATE_LIMIT_PREFIX, user.to_string());
    Symbol::new(env, &key_str)
}

// Helper to get incentive balance key
pub fn get_incentive_balance_key(env: &Env, provider: &Address) -> Symbol {
    let key_str = format!("{}{}", INCENTIVE_BALANCE_PREFIX, provider.to_string());
    Symbol::new(env, &key_str)
}