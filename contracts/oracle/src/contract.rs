use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, String, Vec, Map};
use crate::types::*;
use crate::errors::*;
use crate::storage_keys::*;
use crate::price_aggregator::PriceAggregator;
use crate::circuit_breaker::CircuitBreaker;
use crate::rate_limiter::RateLimiter;
use crate::incentives::IncentiveManager;

#[contract]
pub struct OracleContract;

#[contractimpl]
impl OracleContract {
    /// Initialize the oracle contract with admin and treasury
    pub fn initialize(env: Env, admin: Address, treasury: Address) {
        // Check if already initialized
        if env.storage().instance().has(&Symbol::new(&env, ADMIN)) {
            already_initialized();
        }

        admin.require_auth();

        // Set up core storage
        env.storage().instance().set(&Symbol::new(&env, ADMIN), &admin);
        env.storage().instance().set(&Symbol::new(&env, TREASURY), &treasury);
        env.storage().instance().set(&Symbol::new(&env, PROVIDER_COUNT), &0u64);
        env.storage().instance().set(&Symbol::new(&env, FEED_COUNT), &0u64);
        env.storage().instance().set(&Symbol::new(&env, "dist_cnt"), &0u64);
        env.storage().instance().set(&Symbol::new(&env, BASE_REWARD_RATE), &100i128); // Base reward in stroops

        env.events().publish(
            (Symbol::new(&env, "contract_initialized"),),
            (admin, treasury, env.ledger().timestamp()),
        );
    }

    /// Verify admin authorization
    fn verify_admin(env: &Env, caller: &Address) {
        let admin: Address = env.storage()
            .instance()
            .get(&Symbol::new(env, ADMIN))
            .expect("Admin not set");
        
        if caller != &admin {
            unauthorized();
        }
    }

    /// Register a new oracle provider
    pub fn register_provider(
        env: Env,
        admin: Address,
        provider_address: Address,
        provider_type: OracleProviderType,
        is_primary: bool,
        stake_amount: i128,
        min_stake: i128,
    ) {
        admin.require_auth();
        Self::verify_admin(&env, &admin);

        if stake_amount < min_stake {
            insufficient_stake();
        }

        // Check if provider already exists
        let p_key = get_provider_key(&env, &provider_address);
        if env.storage().instance().has(&p_key) {
            provider_already_exists();
        }

        let provider = OracleProvider {
            address: provider_address.clone(),
            provider_type,
            is_primary,
            is_active: true,
            reputation_score: 50, // Start with middle reputation
            total_updates: 0,
            successful_updates: 0,
            staked_amount: stake_amount,
            created_at: env.ledger().timestamp(),
        };

        env.storage().instance().set(&p_key, &provider);

        // Increment provider counter
        let mut count: u64 = env.storage().instance().get(&Symbol::new(&env, PROVIDER_COUNT)).unwrap_or(0);
        count += 1;
        env.storage().instance().set(&Symbol::new(&env, PROVIDER_COUNT), &count);

        // Initialize provider health tracking
        let ph_key = get_provider_health_key(&env, &provider_address);
        let health = ProviderHealth {
            provider: provider_address.clone(),
            consecutive_failures: 0,
            last_successful_update: 0,
            price_deviation_count: 0,
            availability_score: 1000,
        };
        env.storage().instance().set(&ph_key, &health);

        env.events().publish(
            (Symbol::new(&env, "provider_registered"),),
            (provider_address, provider_type as u32, is_primary, stake_amount),
        );
    }

    /// Create a new price feed
    pub fn create_price_feed(
        env: Env,
        admin: Address,
        feed_id: Symbol,
        asset_symbol: String,
        description: String,
        decimals: u32,
        min_update_interval: u64,
        max_staleness: u64,
        max_price_change_bps: u32,
        providers: Vec<Address>,
        enable_circuit_breaker: bool,
    ) {
        admin.require_auth();
        Self::verify_admin(&env, &admin);

        // Check if feed already exists
        let feed_key = get_feed_key(&env, &feed_id);
        if env.storage().instance().has(&feed_key) {
            feed_already_exists();
        }

        if providers.len() < 2 {
            invalid_input(); // Need at least 2 providers for redundancy
        }

        let feed = PriceFeed {
            feed_id: feed_id.clone(),
            asset_symbol,
            description,
            decimals,
            min_update_interval,
            max_staleness,
            is_active: true,
            circuit_breaker_enabled: enable_circuit_breaker,
            max_price_change_bps,
            providers,
            created_at: env.ledger().timestamp(),
        };

        env.storage().instance().set(&feed_key, &feed);

        // Initialize price history storage
        let history_key = get_price_history_key(&env, &feed_id);
        env.storage().instance().set(&history_key, &Vec::<HistoricalPrice>::new(&env));

        // Initialize circuit breaker if enabled
        if enable_circuit_breaker {
            CircuitBreaker::initialize(&env, &feed_id, 3600); // 1 hour cooldown
        }

        // Initialize fallback configuration
        let fb_key = get_fallback_config_key(&env, &feed_id);
        let fallback_config = FallbackConfig {
            enabled: true,
            fallback_providers: Vec::new(&env),
            primary_failure_threshold: 3,
            current_failures: 0,
            using_fallback: false,
        };
        env.storage().instance().set(&fb_key, &fallback_config);

        // Increment feed counter
        let mut count: u64 = env.storage().instance().get(&Symbol::new(&env, FEED_COUNT)).unwrap_or(0);
        count += 1;
        env.storage().instance().set(&Symbol::new(&env, FEED_COUNT), &count);

        env.events().publish(
            (Symbol::new(&env, "feed_created"), feed_id),
            (asset_symbol, decimals, providers.len() as u32, enable_circuit_breaker),
        );
    }

    /// Submit a price update from an authorized provider
    pub fn submit_price(
        env: Env,
        provider: Address,
        feed_id: Symbol,
        price: i128,
    ) {
        provider.require_auth();

        // Validate inputs
        if price <= 0 {
            invalid_price();
        }

        // Verify provider is authorized for this feed
        let feed_key = get_feed_key(&env, &feed_id);
        let feed: PriceFeed = env.storage().instance().get(&feed_key).unwrap_or_else(|| feed_not_found());
        
        if !feed.is_active {
            feed_inactive();
        }

        // Check if provider is authorized
        let mut is_authorized = false;
        for p in feed.providers.iter() {
            if p == &provider {
                is_authorized = true;
                break;
            }
        }
        if !is_authorized {
            unauthorized();
        }

        // Get provider data to ensure they're active
        let p_key = get_provider_key(&env, &provider);
        let mut provider_data: OracleProvider = env.storage().instance().get(&p_key).unwrap_or_else(|| provider_not_found());
        if !provider_data.is_active {
            provider_inactive();
        }

        // Check if enough time has passed since last update
        let lp_key = get_latest_price_key(&env, &feed_id);
        if let Some(last_entry) = env.storage().instance().get::<PriceEntry>(&lp_key) {
            if env.ledger().timestamp() - last_entry.timestamp < feed.min_update_interval {
                update_too_early();
            }
        }

        // Check circuit breaker
        if CircuitBreaker::is_triggered(&env, &feed_id) {
            circuit_breaker_triggered();
        }

        // Get previous aggregated price for circuit breaker check
        let mut previous_price = if let Some(last_price) = env.storage().instance().get::<PriceEntry>(&lp_key) {
            last_price.price
        } else {
            0
        };

        // Create new price entry
        let new_entry = PriceEntry {
            price,
            timestamp: env.ledger().timestamp(),
            provider: provider.clone(),
            provider_type: provider_data.provider_type,
            block_number: env.ledger().sequence(),
        };

        // Store latest price
        env.storage().instance().set(&lp_key, &new_entry);

        // Add to historical records
        let history_key = get_price_history_key(&env, &feed_id);
        let mut history: Vec<HistoricalPrice> = env.storage().instance().get(&history_key).unwrap_or_else(|| Vec::new(&env));
        
        // Keep history limited to last 1000 entries to prevent storage bloat
        if history.len() >= 1000 {
            // Remove oldest entry
            let mut new_history = Vec::new(&env);
            for i in 1..history.len() {
                new_history.push_back(history.get(i).unwrap().clone());
            }
            history = new_history;
        }
        
        history.push_back(HistoricalPrice {
            price,
            timestamp: env.ledger().timestamp(),
            block_number: env.ledger().sequence(),
            aggregated: false,
        });
        env.storage().instance().set(&history_key, &history);

        // Check if this price would trigger circuit breaker
        if previous_price > 0 {
            let (should_trigger, change_bps) = CircuitBreaker::check_price_movement(
                &env,
                &feed,
                price,
                previous_price,
            );

            if should_trigger {
                CircuitBreaker::trigger(
                    &env,
                    &feed_id,
                    price,
                    previous_price,
                    change_bps,
                    3600,
                );
                // Record failure for this provider
                IncentiveManager::record_failed_update(&env, &provider);
                return;
            }
        }

        // Record successful update
        IncentiveManager::record_successful_update(&env, &provider);
        provider_data.successful_updates += 1;
        provider_data.total_updates += 1;
        env.storage().instance().set(&p_key, &provider_data);

        // Distribute reward
        let base_reward: i128 = env.storage().instance().get(&Symbol::new(&env, BASE_REWARD_RATE)).unwrap_or(100);
        let mut dist_counter: u64 = env.storage().instance().get(&Symbol::new(&env, "dist_cnt")).unwrap_or(0);
        IncentiveManager::distribute_update_reward(&env, &provider, &feed_id, base_reward, &mut dist_counter);
        env.storage().instance().set(&Symbol::new(&env, "dist_cnt"), &dist_counter);

        env.events().publish(
            (Symbol::new(&env, "price_updated"), feed_id),
            (price, env.ledger().timestamp(), provider),
        );
    }

    /// Get aggregated price for a feed
    pub fn get_aggregated_price(env: Env, caller: Address, feed_id: Symbol) -> AggregatedPrice {
        // Apply rate limiting
        RateLimiter::consume_query(&env, &caller, &feed_id, 3600, 1000);

        // Get feed data
        let feed_key = get_feed_key(&env, &feed_id);
        let feed: PriceFeed = env.storage().instance().get(&feed_key).unwrap_or_else(|| feed_not_found());
        
        if !feed.is_active {
            feed_inactive();
        }

        // Check circuit breaker
        if CircuitBreaker::is_triggered(&env, &feed_id) {
            circuit_breaker_triggered();
        }

        // Collect all recent prices from providers
        let mut prices = Vec::new(&env);
        for provider in feed.providers.iter() {
            let lp_key = get_latest_price_key(&env, &feed_id);
            if let Some(price_entry) = env.storage().instance().get::<PriceEntry>(&lp_key) {
                if price_entry.provider == *provider {
                    prices.push_back(price_entry);
                }
            }
        }

        // Check if we need to use fallback providers
        let fb_key = get_fallback_config_key(&env, &feed_id);
        let mut fallback_config = env.storage().instance().get::<FallbackConfig>(&fb_key).unwrap();
        
        if prices.len() < 2 && fallback_config.enabled {
            // Switch to fallback providers
            fallback_config.using_fallback = true;
            fallback_config.current_failures += 1;
            env.storage().instance().set(&fb_key, &fallback_config);

            // Add fallback prices
            for fallback_provider in fallback_config.fallback_providers.iter() {
                let lp_key = get_latest_price_key(&env, &feed_id);
                if let Some(price_entry) = env.storage().instance().get::<PriceEntry>(&lp_key) {
                    if price_entry.provider == *fallback_provider {
                        prices.push_back(price_entry);
                    }
                }
            }
        } else {
            fallback_config.using_fallback = false;
            fallback_config.current_failures = 0;
            env.storage().instance().set(&fb_key, &fallback_config);
        }

        // Remove outliers and aggregate
        let filtered_prices = PriceAggregator::remove_outliers(&env, prices);
        
        // Need min 2 sources for aggregation
        let aggregated = PriceAggregator::aggregate_prices(
            &env,
            &filtered_prices,
            2,
            feed.max_staleness,
        );

        // Check if price is stale
        if !aggregated.is_fresh {
            stale_price();
        }

        aggregated
    }

    /// Get historical prices
    pub fn get_historical_prices(
        env: Env,
        caller: Address,
        feed_id: Symbol,
        limit: u32,
    ) -> Vec<HistoricalPrice> {
        RateLimiter::consume_query(&env, &caller, &feed_id, 3600, 1000);

        if limit > 500 {
            invalid_input();
        }

        let history_key = get_price_history_key(&env, &feed_id);
        let history: Vec<HistoricalPrice> = env.storage().instance().get(&history_key).unwrap_or_else(|| Vec::new(&env));

        // Return most recent entries
        let mut result = Vec::new(&env);
        let start_idx = if history.len() > limit as usize {
            history.len() - limit as usize
        } else {
            0
        };

        for i in start_idx..history.len() {
            result.push_back(history.get(i).unwrap().clone());
        }

        result
    }

    /// Create custom data feed (for non-price data)
    pub fn create_custom_feed(
        env: Env,
        admin: Address,
        feed_id: Symbol,
        description: String,
        data_type: String,
        max_staleness: u64,
        providers: Vec<Address>,
    ) {
        admin.require_auth();
        Self::verify_admin(&env, &admin);

        let cf_key = get_custom_feed_key(&env, &feed_id);
        if env.storage().instance().has(&cf_key) {
            feed_already_exists();
        }

        let custom_feed = CustomDataFeed {
            feed_id: feed_id.clone(),
            description,
            data_type,
            is_active: true,
            authorized_providers: providers,
            last_update: 0,
            max_staleness,
        };

        env.storage().instance().set(&cf_key, &custom_feed);

        // Initialize data storage
        let cd_key = get_custom_data_key(&env, &feed_id);
        env.storage().instance().set(&cd_key, &Vec::<CustomDataEntry>::new(&env));

        env.events().publish(
            (Symbol::new(&env, "custom_feed_created"), feed_id),
            (data_type, custom_feed.authorized_providers.len() as u32),
        );
    }

    /// Submit custom data
    pub fn submit_custom_data(
        env: Env,
        provider: Address,
        feed_id: Symbol,
        data: String,
    ) {
        provider.require_auth();

        let cf_key = get_custom_feed_key(&env, &feed_id);
        let mut feed: CustomDataFeed = env.storage().instance().get(&cf_key).unwrap_or_else(|| feed_not_found());

        // Verify authorization
        let mut is_authorized = false;
        for p in feed.authorized_providers.iter() {
            if p == &provider {
                is_authorized = true;
                break;
            }
        }
        if !is_authorized {
            unauthorized();
        }

        // Store data
        let cd_key = get_custom_data_key(&env, &feed_id);
        let mut history: Vec<CustomDataEntry> = env.storage().instance().get(&cd_key).unwrap_or_else(|| Vec::new(&env));
        
        history.push_back(CustomDataEntry {
            data,
            timestamp: env.ledger().timestamp(),
            provider: provider.clone(),
            block_number: env.ledger().sequence(),
        });

        // Keep history limited
        if history.len() > 500 {
            let mut new_history = Vec::new(&env);
            for i in 1..history.len() {
                new_history.push_back(history.get(i).unwrap().clone());
            }
            history = new_history;
        }

        env.storage().instance().set(&cd_key, &history);
        feed.last_update = env.ledger().timestamp();
        env.storage().instance().set(&cf_key, &feed);

        // Record success and reward
        IncentiveManager::record_successful_update(&env, &provider);

        env.events().publish(
            (Symbol::new(&env, "custom_data_updated"), feed_id),
            (env.ledger().timestamp(), provider),
        );
    }

    /// Get custom data
    pub fn get_custom_data(env: Env, caller: Address, feed_id: Symbol) -> Option<CustomDataEntry> {
        RateLimiter::consume_query(&env, &caller, &feed_id, 3600, 1000);

        let cf_key = get_custom_feed_key(&env, &feed_id);
        let feed: CustomDataFeed = env.storage().instance().get(&cf_key).unwrap_or_else(|| feed_not_found());

        if !feed.is_active {
            feed_inactive();
        }

        // Check staleness
        if env.ledger().timestamp() - feed.last_update > feed.max_staleness {
            stale_price();
        }

        let cd_key = get_custom_data_key(&env, &feed_id);
        let history: Vec<CustomDataEntry> = env.storage().instance().get(&cd_key).unwrap_or_else(|| Vec::new(&env));

        if history.len() > 0 {
            Some(history.get(history.len() - 1))
        } else {
            None
        }
    }

    /// Withdraw earned incentives
    pub fn withdraw_incentives(env: Env, provider: Address) -> i128 {
        IncentiveManager::withdraw_incentives(&env, provider)
    }

    /// Reset circuit breaker manually
    pub fn reset_circuit_breaker(env: Env, admin: Address, feed_id: Symbol) {
        admin.require_auth();
        Self::verify_admin(&env, &admin);
        CircuitBreaker::reset(&env, &feed_id);
    }

    /// Add fallback provider to a feed
    pub fn add_fallback_provider(
        env: Env,
        admin: Address,
        feed_id: Symbol,
        provider: Address,
    ) {
        admin.require_auth();
        Self::verify_admin(&env, &admin);

        let fb_key = get_fallback_config_key(&env, &feed_id);
        let mut config = env.storage().instance().get::<FallbackConfig>(&fb_key).unwrap();
        
        // Check if already in fallback list
        for p in config.fallback_providers.iter() {
            if p == &provider {
                return; // Already exists
            }
        }
        
        config.fallback_providers.push_back(provider);
        env.storage().instance().set(&fb_key, &config);
    }

    /// Get circuit breaker state
    pub fn get_circuit_breaker_state(env: Env, feed_id: Symbol) -> Option<CircuitBreakerState> {
        CircuitBreaker::get_state(&env, &feed_id)
    }

    /// Check if price is fresh
    pub fn is_data_fresh(env: Env, feed_id: Symbol) -> bool {
        let feed_key = get_feed_key(&env, &feed_id);
        let feed = env.storage().instance().get::<PriceFeed>(&feed_key).unwrap_or_else(|| feed_not_found());
        
        let lp_key = get_latest_price_key(&env, &feed_id);
        if let Some(last_entry) = env.storage().instance().get::<PriceEntry>(&lp_key) {
            env.ledger().timestamp() - last_entry.timestamp <= feed.max_staleness
        } else {
            false
        }
    }

    /// Get provider's current incentive balance
    pub fn get_provider_balance(env: Env, provider: Address) -> i128 {
        IncentiveManager::get_balance(&env, &provider)
    }
}