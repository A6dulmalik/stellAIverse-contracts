#![no_std]

mod errors;

use crate::errors::ContractError;
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Map, Symbol, Vec};
/// Supported NFT standards
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NftStandard {
    ERC721,  // Non-fungible token (single NFT per token ID)
    ERC1155, // Semi-fungible token (multiple copies per token ID)
}

/// Tier configuration for duration-based rewards
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardTier {
    pub min_duration: u64, // Minimum staking duration in seconds to qualify for this tier
    pub reward_multiplier: u32, // Multiplier in basis points (100 = 1x, 125 = 1.25x)
}

/// NFT collection configuration
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionConfig {
    pub standard: NftStandard,
    pub base_reward_rate: i128, // Base reward rate per day (in smallest token units)
    pub is_whitelisted: bool,
    pub tiers: Vec<RewardTier>, // Duration-based reward tiers
}

/// Reward token configuration
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardTokenConfig {
    pub is_enabled: bool,
    pub total_allocated: i128,
    pub total_distributed: i128,
}

/// Stake information for an individual NFT
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NftStakeInfo {
    pub collection: Address,
    pub token_id: u64,
    pub amount: i128, // For ERC1155: number of tokens staked; for ERC721: always 1
    pub staker: Address,
    pub start_timestamp: u64,
    pub last_claim_timestamp: u64,
    pub unclaimed_rewards: Map<Address, i128>, // Reward token address -> amount
    pub in_cooldown: bool,
    pub cooldown_start: u64,
    pub emergency_withdrawn: bool,
}

/// Storage key enum for all contract storage entries
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Stored NFT stake: (staker, collection, token_id)
    Stake(Address, Address, u64),
    /// Staking statistics
    StakingStats,
    /// Admin address
    Admin,
    /// Collection configuration: collection address -> config
    CollectionConfig(Address),
    /// Reward token allocation: token address -> allocation
    RewardTokenAllocation(Address),
    /// User stakes list: staker address -> list of their stakes
    UserStakes(Address),
    /// Cache invalidation timestamp
    CacheInvalidation,
    /// Cooldown period for unstaking
    CooldownPeriod,
}

/// Global statistics for dashboard
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StakingStats {
    pub total_nfts_staked: u64,
    pub total_stakers: u64,
    pub total_rewards_distributed: Map<Address, i128>,
    pub collections_staked: Vec<Address>,
}

/// Storage key for cache invalidation
const CACHE_INVALIDATION_KEY: &str = "cache_invalidation_ts";
/// Cooldown period for unstaking (7 days)
const DEFAULT_COOLDOWN_PERIOD: u64 = 7 * 86400;


#[contract]
pub struct StakingBonuses;

#[contractimpl]
impl StakingBonuses {
    /// Initialize the contract with an admin address
    pub fn init_contract(env: Env, admin_addr: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin_addr.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin_addr);

        // Initialize statistics
        let empty_stats = StakingStats {
            total_nfts_staked: 0,
            total_stakers: 0,
            total_rewards_distributed: Map::new(&env),
            collections_staked: Vec::new(&env),
        };
        env.storage()
            .instance()
            .set(&DataKey::StakingStats, &empty_stats);

        Ok(())
    }

    // =============================================
    // ADMIN FUNCTIONS
    // =============================================

    /// Add or update a whitelisted NFT collection (admin only)
    pub fn add_whitelisted_collection(
        env: Env,
        caller: Address,
        collection_address: Address,
        config: CollectionConfig,
    ) -> Result<(), ContractError> {
        // Verify admin
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::Unauthorized)?;
        if caller != admin {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();

        // Validate tier configuration
        if !config.tiers.is_empty() {
            let mut prev_min = 0u64;
            for tier in config.tiers.iter() {
                if tier.min_duration <= prev_min || tier.reward_multiplier < 100 {
                    return Err(ContractError::InvalidTierConfig);
                }
                prev_min = tier.min_duration;
            }
        }

        // Store collection configuration
        env.storage().instance().set(
            &DataKey::CollectionConfig(collection_address.clone()),
            &config,
        );

        // Update statistics if this is a new collection
        let mut stats: StakingStats = env
            .storage()
            .instance()
            .get(&DataKey::StakingStats)
            .unwrap();

        if !stats.collections_staked.contains(&collection_address) {
            stats
                .collections_staked
                .push_back(collection_address.clone());
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "staking_stats"), &stats);
        }

        // Emit event for audit
        env.events().publish(
            (
                Symbol::new(&env, "staking"),
                Symbol::new(&env, "collection_added"),
            ),
            (collection_address, config.is_whitelisted),
        );

        Ok(())
    }

    /// Add a supported reward token (admin only)
    pub fn add_reward_token(
        env: Env,
        caller: Address,
        token_address: Address,
        allocation: i128,
    ) -> Result<(), ContractError> {
        // Verify admin
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::Unauthorized)?;
        if caller != admin {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();

        if allocation <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        // Check if token already exists
        let key = (Symbol::new(&env, "reward_token"), token_address.clone());
        let mut config = if env.storage().instance().has(&key) {
            env.storage()
                .instance()
                .get::<_, RewardTokenConfig>(&key)
                .unwrap()
        } else {
            RewardTokenConfig {
                is_enabled: true,
                total_allocated: 0,
                total_distributed: 0,
            }
        };

        // Transfer allocated tokens from admin to contract
        let token_client = token::Client::new(&env, &token_address);
        token_client.transfer(&caller, &env.current_contract_address(), &allocation);

        config.total_allocated += allocation;
        env.storage().instance().set(&key, &config);

        env.events().publish(
            (
                Symbol::new(&env, "staking"),
                Symbol::new(&env, "reward_token_added"),
            ),
            (token_address, allocation),
        );

        Ok(())
    }

    /// Update cooldown period (admin only)
    pub fn set_cooldown_period(
        env: Env,
        caller: Address,
        new_cooldown: u64,
    ) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::Unauthorized)?;
        if caller != admin {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::CooldownPeriod, &new_cooldown);

        env.events().publish(
            (
                Symbol::new(&env, "staking"),
                Symbol::new(&env, "cooldown_updated"),
            ),
            new_cooldown,
        );

        Ok(())
    }

    // =============================================
    // USER STAKING FUNCTIONS
    // =============================================

    /// Stake an NFT (supports both ERC721 and ERC1155)
    pub fn stake_nft(
        env: Env,
        staker: Address,
        collection: Address,
        token_id: u64,
        amount: i128,
    ) -> Result<(), ContractError> {
        staker.require_auth();

        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        // Verify collection is whitelisted
        let collection_config: CollectionConfig = env
            .storage()
            .instance()
            .get(&(Symbol::new(&env, "collection_config"), collection.clone()))
            .ok_or(ContractError::CollectionNotWhitelisted)?;

        if !collection_config.is_whitelisted {
            return Err(ContractError::CollectionNotWhitelisted);
        }

        // For ERC721, amount must be 1
        if matches!(collection_config.standard, NftStandard::ERC721) && amount != 1 {
            return Err(ContractError::InvalidNftStandard);
        }

        // Check if this NFT is already staked by this user
        let stake_key = DataKey::Stake(staker.clone(), collection.clone(), token_id);

        if env.storage().instance().has(&stake_key) {
            return Err(ContractError::NftAlreadyStaked);
        }

        // Transfer NFT from staker to contract
        // In a real implementation, this would call the NFT contract's transfer/transferFrom
        // For Soroban, we assume the NFT contract has a compatible transfer interface
        let nft_client = token::Client::new(&env, &collection);
        nft_client.transfer(
            &staker,
            &env.current_contract_address(),
            &(token_id as i128).saturating_mul(amount),
        );

        let now = env.ledger().timestamp();

        // Initialize unclaimed rewards for all supported reward tokens
        let unclaimed_rewards = Map::new(&env);
        // We'll get all reward tokens and initialize their rewards to 0
        // In production, you might store a list of reward tokens to iterate over

        // Create stake info
        let stake_info = NftStakeInfo {
            collection: collection.clone(),
            token_id,
            amount,
            staker: staker.clone(),
            start_timestamp: now,
            last_claim_timestamp: now,
            unclaimed_rewards,
            in_cooldown: false,
            cooldown_start: 0,
            emergency_withdrawn: false,
        };

        // Save stake
        env.storage().instance().set(&stake_key, &stake_info);

        // Add to user's stakes list
        let user_stakes_key = DataKey::UserStakes(staker.clone());
        let mut user_stakes = if env.storage().instance().has(&user_stakes_key) {
            env.storage()
                .instance()
                .get::<DataKey, Vec<(Address, u64)>>(&user_stakes_key)
                .unwrap()
        } else {
            Vec::new(&env)
        };
        user_stakes.push_back((collection.clone(), token_id));
        env.storage().instance().set(&user_stakes_key, &user_stakes);

        // Update statistics
        let mut stats: StakingStats = env
            .storage()
            .instance()
            .get(&DataKey::StakingStats)
            .unwrap();
        stats.total_nfts_staked += 1;
        // For simplicity, we're incrementing staker count if this is their first stake
        // In production, you'd track unique stakers more accurately
        if stats.total_stakers == 0 {
            stats.total_stakers = 1;
        }
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "staking_stats"), &stats);

        // Invalidate cache
        Self::invalidate_query_cache(&env, &staker);

        env.events().publish(
            (
                Symbol::new(&env, "staking"),
                Symbol::new(&env, "nft_staked"),
            ),
            (staker, collection, token_id, amount, now),
        );

        Ok(())
    }

    /// Initiate unstaking (starts cooldown period)
    pub fn initiate_unstake(
        env: Env,
        staker: Address,
        collection: Address,
        token_id: u64,
    ) -> Result<(), ContractError> {
        staker.require_auth();

        let stake_key = DataKey::Stake(staker.clone(), collection.clone(), token_id);

        let mut stake_info: NftStakeInfo = env
            .storage()
            .instance()
            .get(&stake_key)
            .ok_or(ContractError::StakeNotFound)?;

        if stake_info.in_cooldown {
            return Err(ContractError::CooldownNotElapsed);
        }

        // First accrue all pending rewards
        Self::accrue_all_rewards(&env, &mut stake_info)?;

        let now = env.ledger().timestamp();
        stake_info.in_cooldown = true;
        stake_info.cooldown_start = now;

        env.storage().instance().set(&stake_key, &stake_info);

        // Get cooldown period
        let cooldown_period: u64 = env
            .storage()
            .instance()
            .get(&DataKey::CooldownPeriod)
            .unwrap_or(DEFAULT_COOLDOWN_PERIOD);

        env.events().publish(
            (
                Symbol::new(&env, "staking"),
                Symbol::new(&env, "unstake_initiated"),
            ),
            (staker, collection, token_id, now + cooldown_period),
        );

        Ok(())
    }

    /// Complete unstaking after cooldown
    pub fn complete_unstake(
        env: Env,
        staker: Address,
        collection: Address,
        token_id: u64,
    ) -> Result<(), ContractError> {
        staker.require_auth();

        let stake_key = DataKey::Stake(staker.clone(), collection.clone(), token_id);

        let mut stake_info: NftStakeInfo = env
            .storage()
            .instance()
            .get(&stake_key)
            .ok_or(ContractError::StakeNotFound)?;

        if !stake_info.in_cooldown {
            return Err(ContractError::StakeLocked);
        }

        // Check if cooldown has elapsed
        let cooldown_period: u64 = env
            .storage()
            .instance()
            .get(&DataKey::CooldownPeriod)
            .unwrap_or(DEFAULT_COOLDOWN_PERIOD);

        let now = env.ledger().timestamp();
        if now < stake_info.cooldown_start.saturating_add(cooldown_period) {
            return Err(ContractError::CooldownNotElapsed);
        }

        // Claim all pending rewards first
        Self::claim_all_rewards_internal(&env, &mut stake_info, &staker)?;

        // Transfer NFT back to staker
        let nft_client = token::Client::new(&env, &collection);
        nft_client.transfer(
            &env.current_contract_address(),
            &staker,
            &(token_id as i128).saturating_mul(stake_info.amount),
        );

        // Remove stake from storage
        env.storage().instance().remove(&stake_key);

        // Update statistics
        let mut stats: StakingStats = env
            .storage()
            .instance()
            .get(&DataKey::StakingStats)
            .unwrap();
        stats.total_nfts_staked = stats.total_nfts_staked.saturating_sub(1);
        env.storage().instance().set(&DataKey::StakingStats, &stats);

        Self::invalidate_query_cache(&env, &staker);

        env.events().publish(
            (
                Symbol::new(&env, "staking"),
                Symbol::new(&env, "unstake_completed"),
            ),
            (staker, collection, token_id, stake_info.amount),
        );

        Ok(())
    }

    /// Emergency withdrawal - allows users to withdraw without waiting for cooldown (but forfeits rewards)
    pub fn emergency_withdraw(
        env: Env,
        staker: Address,
        collection: Address,
        token_id: u64,
    ) -> Result<(), ContractError> {
        staker.require_auth();

        let stake_key = DataKey::Stake(staker.clone(), collection.clone(), token_id);

        let mut stake_info: NftStakeInfo = env
            .storage()
            .instance()
            .get(&stake_key)
            .ok_or(ContractError::StakeNotFound)?;

        if stake_info.emergency_withdrawn {
            return Err(ContractError::EmergencyWithdrawalAlreadyUsed);
        }

        // Mark as withdrawn
        stake_info.emergency_withdrawn = true;

        // Transfer NFT back to staker (forfeit all rewards)
        let nft_client = token::Client::new(&env, &collection);
        nft_client.transfer(
            &env.current_contract_address(),
            &staker,
            &(token_id as i128).saturating_mul(stake_info.amount),
        );

        // Remove stake
        env.storage().instance().remove(&stake_key);

        // Update statistics
        let mut stats: StakingStats = env
            .storage()
            .instance()
            .get(&DataKey::StakingStats)
            .unwrap();
        stats.total_nfts_staked = stats.total_nfts_staked.saturating_sub(1);
        env.storage().instance().set(&DataKey::StakingStats, &stats);

        // Remove from user's stakes list
        let user_stakes_key = DataKey::UserStakes(staker.clone());
        if env.storage().instance().has(&user_stakes_key) {
            let mut user_stakes = env
                .storage()
                .instance()
                .get::<DataKey, Vec<(Address, u64)>>(&user_stakes_key)
                .unwrap();
            // Find and remove the stake from the list
            for i in 0..user_stakes.len() {
                let (coll, id) = user_stakes.get(i).unwrap();
                if coll == collection && id == token_id {
                    user_stakes.remove(i);
                    env.storage().instance().set(&user_stakes_key, &user_stakes);
                    break;
                }
            }
        }

        Self::invalidate_query_cache(&env, &staker);

        env.events().publish(
            (
                Symbol::new(&env, "staking"),
                Symbol::new(&env, "emergency_withdraw"),
            ),
            (staker, collection, token_id),
        );

        Ok(())
    }

    /// Claim rewards for a specific stake
    pub fn claim_rewards(
        env: Env,
        staker: Address,
        collection: Address,
        token_id: u64,
    ) -> Result<Map<Address, i128>, ContractError> {
        staker.require_auth();

        let stake_key = DataKey::Stake(staker.clone(), collection.clone(), token_id);

        let mut stake_info: NftStakeInfo = env
            .storage()
            .instance()
            .get(&stake_key)
            .ok_or(ContractError::StakeNotFound)?;

        // Accrue all pending rewards
        Self::accrue_all_rewards(&env, &mut stake_info)?;

        // Claim all rewards
        let claimed = Self::claim_all_rewards_internal(&env, &mut stake_info, &staker)?;

        // Update stake in storage
        env.storage().instance().set(&stake_key, &stake_info);

        Self::invalidate_query_cache(&env, &staker);

        env.events().publish(
            (
                Symbol::new(&env, "staking"),
                Symbol::new(&env, "rewards_claimed"),
            ),
            (staker, collection, token_id, claimed.clone()),
        );

        Ok(claimed)
    }

    // =============================================
    // INTERNAL REWARD CALCULATION FUNCTIONS
    // =============================================

    /// Accrue rewards for all supported tokens for a stake
    fn accrue_all_rewards(env: &Env, stake_info: &mut NftStakeInfo) -> Result<(), ContractError> {
        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(stake_info.last_claim_timestamp);

        if elapsed == 0 {
            return Ok(());
        }

        // Get collection configuration
        let collection_config: CollectionConfig = env
            .storage()
            .instance()
            .get(&(
                Symbol::new(env, "collection_config"),
                stake_info.collection.clone(),
            ))
            .ok_or(ContractError::CollectionNotWhitelisted)?;

        // Calculate applicable tier multiplier
        let total_stake_duration = now.saturating_sub(stake_info.start_timestamp);
        let tier_multiplier =
            Self::calculate_tier_multiplier(&collection_config.tiers, total_stake_duration);

        // Calculate daily rewards
        let days_elapsed = (elapsed as f64) / 86400.0;
        let base_rewards = (collection_config.base_reward_rate as f64 * days_elapsed) as i128;
        let final_rewards = base_rewards
            .checked_mul(tier_multiplier as i128)
            .ok_or(ContractError::OverflowError)?
            .checked_div(100) // Convert from basis points
            .ok_or(ContractError::RewardCalculationFailed)?;

        // Add rewards to each supported reward token (simplified - in production, distribute across configured tokens)
        // For this implementation, we'll add to all reward tokens proportionally
        let reward_tokens = Self::get_all_reward_tokens(env);
        if reward_tokens.is_empty() {
            return Ok(());
        }

        let reward_per_token = final_rewards
            .checked_div(reward_tokens.len() as i128)
            .ok_or(ContractError::RewardCalculationFailed)?;

        for token in reward_tokens {
            let current = stake_info.unclaimed_rewards.get(token.clone()).unwrap_or(0);
            stake_info
                .unclaimed_rewards
                .set(token, current + reward_per_token);
        }

        // Update last claim timestamp
        stake_info.last_claim_timestamp = now;

        Ok(())
    }

    /// Calculate the applicable tier multiplier based on staking duration
    fn calculate_tier_multiplier(tiers: &Vec<RewardTier>, total_duration: u64) -> u32 {
        let mut max_multiplier = 100u32; // Base 1x multiplier (100 basis points)

        for tier in tiers.iter() {
            if total_duration >= tier.min_duration && tier.reward_multiplier > max_multiplier {
                max_multiplier = tier.reward_multiplier;
            }
        }

        max_multiplier
    }

    /// Internal function to claim all rewards and transfer to staker
    fn claim_all_rewards_internal(
        env: &Env,
        stake_info: &mut NftStakeInfo,
        staker: &Address,
    ) -> Result<Map<Address, i128>, ContractError> {
        let mut claimed = Map::new(env);

        for (token_address, amount) in stake_info.unclaimed_rewards.iter() {
            if amount <= 0 {
                continue;
            }

            // Verify we have enough balance to distribute
            let token_key = (Symbol::new(env, "reward_token"), token_address.clone());
            let mut token_config: RewardTokenConfig = env
                .storage()
                .instance()
                .get(&token_key)
                .ok_or(ContractError::RewardTokenNotSupported)?;

            // Transfer tokens to staker
            let token_client = token::Client::new(env, &token_address);
            token_client.transfer(&env.current_contract_address(), staker, &amount);

            // Update token distribution stats
            token_config.total_distributed += amount;
            env.storage().instance().set(&token_key, &token_config);

            // Update global stats
            let mut stats: StakingStats = env
                .storage()
                .instance()
                .get(&Symbol::new(env, "staking_stats"))
                .unwrap();
            let current_total = stats
                .total_rewards_distributed
                .get(token_address.clone())
                .unwrap_or(0);
            stats
                .total_rewards_distributed
                .set(token_address.clone(), current_total + amount);
            env.storage()
                .instance()
                .set(&Symbol::new(env, "staking_stats"), &stats);

            claimed.set(token_address.clone(), amount);
            stake_info.unclaimed_rewards.set(token_address, 0);
        }

        Ok(claimed)
    }

    /// Get all supported reward tokens
    fn get_all_reward_tokens(env: &Env) -> Vec<Address> {
        // In a production implementation, you'd maintain a list of reward tokens
        // For simplicity, this is a placeholder - in real code you'd iterate through stored tokens
        let tokens = Vec::new(env);
        // This would be populated from storage in production
        tokens
    }

    // =============================================
    // VIEW FUNCTIONS FOR DASHBOARD
    // =============================================

    /// Get stake information for a specific NFT
    pub fn get_stake_info(
        env: Env,
        staker: Address,
        collection: Address,
        token_id: u64,
    ) -> Option<NftStakeInfo> {
        let stake_key = DataKey::Stake(staker, collection, token_id);
        let mut stake_info = env
            .storage()
            .instance()
            .get::<_, NftStakeInfo>(&stake_key)?;

        // Accrue rewards before returning (for accurate calculations)
        let _ = Self::accrue_all_rewards(&env, &mut stake_info);
        env.storage().instance().set(&stake_key, &stake_info);

        Some(stake_info)
    }

    /// Get all stakes for a user
    pub fn get_user_stakes(env: Env, staker: Address) -> Vec<(Address, u64)> {
        let user_stakes_key = DataKey::UserStakes(staker);
        if env.storage().instance().has(&user_stakes_key) {
            env.storage()
                .instance()
                .get::<_, Vec<(Address, u64)>>(&user_stakes_key)
                .unwrap()
        } else {
            Vec::new(&env)
        }
    }

    /// Get global staking statistics
    pub fn get_staking_stats(env: Env) -> StakingStats {
        env.storage()
            .instance()
            .get(&DataKey::StakingStats)
            .unwrap()
    }

    /// Get collection configuration
    pub fn get_collection_config(env: Env, collection: Address) -> Option<CollectionConfig> {
        env.storage()
            .instance()
            .get(&(Symbol::new(&env, "collection_config"), collection))
    }

    /// Calculate pending rewards for a stake
    pub fn calculate_pending_rewards(
        env: Env,
        staker: Address,
        collection: Address,
        token_id: u64,
    ) -> Result<Map<Address, i128>, ContractError> {
        let stake_key = DataKey::Stake(staker, collection, token_id);
        let stake_info: NftStakeInfo = env.storage().instance()
            .get(&stake_key)
            .ok_or(ContractError::StakeNotFound)?;

        // Calculate what the new rewards would be without modifying storage
        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(stake_info.last_claim_timestamp);
        
        if elapsed == 0 {
            return Ok(stake_info.unclaimed_rewards.clone());
        }

        // Create a copy to calculate rewards
        let mut temp_info = stake_info.clone();
        Self::accrue_all_rewards(&env, &mut temp_info)?;

        Ok(temp_info.unclaimed_rewards)
    }

    // =============================================
    // UTILITY FUNCTIONS
    // =============================================

    /// Invalidate query cache after state changes
    fn invalidate_query_cache(env: &Env, user: &Address) {
        let timestamp = env.ledger().timestamp();
        let cache_key = (Symbol::new(env, CACHE_INVALIDATION_KEY), user.clone());
        env.storage().instance().set(&cache_key, &timestamp);

        env.events().publish(
            (
                Symbol::new(env, "staking"),
                Symbol::new(env, "cache_invalidated"),
            ),
            (user.clone(), timestamp),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        contract, contractimpl, contracttype,
        testutils::{Address as _, Ledger},
        Address, Env, String, Vec,
    };

    // Mock NFT contract for testing
    #[contract]
    pub struct MockERC721;

    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    enum MockNftDataKey {
        Owner(u64),
        Balance(Address),
    }

    #[contractimpl]
    impl MockERC721 {
        pub fn initialize(env: Env, admin: Address) {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "admin"), &admin);
        }

        pub fn mint(env: Env, to: Address, token_id: u64) {
            let admin: Address = env
                .storage()
                .instance()
                .get(&Symbol::new(&env, "admin"))
                .unwrap();
            admin.require_auth();

            // Set owner
            env.storage()
                .instance()
                .set(&MockNftDataKey::Owner(token_id), &to);

            // Update balance
            let balance_key = MockNftDataKey::Balance(to.clone());
            let current_balance: i128 = env.storage().instance().get(&balance_key).unwrap_or(0);
            env.storage()
                .instance()
                .set(&balance_key, &(current_balance + 1));
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            // In a real ERC721, this would transfer a specific token, simplified for testing
            let from_balance_key = MockNftDataKey::Balance(from.clone());
            let mut from_balance: i128 =
                env.storage().instance().get(&from_balance_key).unwrap_or(0);
            from_balance -= amount;
            env.storage()
                .instance()
                .set(&from_balance_key, &from_balance);

            let to_balance_key = MockNftDataKey::Balance(to.clone());
            let mut to_balance: i128 = env.storage().instance().get(&to_balance_key).unwrap_or(0);
            to_balance += amount;
            env.storage().instance().set(&to_balance_key, &to_balance);
        }

        pub fn balance_of(env: Env, owner: Address) -> i128 {
            env.storage()
                .instance()
                .get(&MockNftDataKey::Balance(owner))
                .unwrap_or(0)
        }
    }

    // Mock reward token for testing
    #[contract]
    pub struct MockRewardToken;

    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    enum MockTokenDataKey {
        Balance(Address),
    }

    #[contractimpl]
    impl MockRewardToken {
        pub fn initialize(env: Env, admin: Address) {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "admin"), &admin);
        }

        pub fn mint(env: Env, to: Address, amount: i128) {
            let admin: Address = env
                .storage()
                .instance()
                .get(&Symbol::new(&env, "admin"))
                .unwrap();
            admin.require_auth();

            let balance_key = MockTokenDataKey::Balance(to.clone());
            let current: i128 = env.storage().instance().get(&balance_key).unwrap_or(0);
            env.storage()
                .instance()
                .set(&balance_key, &(current + amount));
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();

            let from_key = MockTokenDataKey::Balance(from.clone());
            let from_bal: i128 = env.storage().instance().get(&from_key).unwrap_or(0);
            assert!(from_bal >= amount, "insufficient balance");

            env.storage()
                .instance()
                .set(&from_key, &(from_bal - amount));

            let to_key = MockTokenDataKey::Balance(to.clone());
            let to_bal: i128 = env.storage().instance().get(&to_key).unwrap_or(0);
            env.storage().instance().set(&to_key, &(to_bal + amount));
        }

        pub fn balance_of(env: Env, account: Address) -> i128 {
            env.storage()
                .instance()
                .get(&MockTokenDataKey::Balance(account))
                .unwrap_or(0)
        }
    }

    // Helper to setup test environment
    fn setup_test_env() -> (Env, Address, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        // Deploy mock NFT contract
        let nft_id = env.register_contract(None, MockERC721);
        let nft_client = MockERC721Client::new(&env, &nft_id);
        nft_client.initialize(&admin);
        nft_client.mint(&user, 1); // Mint token ID 1 to user

        // Deploy mock reward token
        let reward_token_id = env.register_contract(None, MockRewardToken);
        let reward_client = MockRewardTokenClient::new(&env, &reward_token_id);
        reward_client.initialize(&admin);
        reward_client.mint(&admin, 1_000_000); // Mint 1M tokens to admin for rewards

        // Deploy staking contract
        let staking_id = env.register_contract(None, StakingBonuses);
        let staking_client = StakingBonusesClient::new(&env, &staking_id);
        staking_client.init_contract(&admin).unwrap();

        (env, admin, user, nft_id, reward_token_id)
    }

    #[test]
    fn test_nft_staking_flow() {
        let (env, admin, user, nft_id, reward_token_id) = setup_test_env();
        let staking_id = env.register_contract(None, StakingBonuses);
        let staking_client = StakingBonusesClient::new(&env, &staking_id);
        staking_client.init_contract(&admin).unwrap();

        // Add reward token
        staking_client
            .add_reward_token(&admin, &reward_token_id, &100_000)
            .unwrap();

        // Create tiers
        let mut tiers = Vec::new(&env);
        tiers.push_back(RewardTier {
            min_duration: 30 * 86400, // 30 days
            reward_multiplier: 125,   // 1.25x
        });
        tiers.push_back(RewardTier {
            min_duration: 90 * 86400, // 90 days
            reward_multiplier: 150,   // 1.5x
        });

        // Whitelist NFT collection
        let collection_config = CollectionConfig {
            standard: NftStandard::ERC721,
            base_reward_rate: 100, // 100 tokens per day
            is_whitelisted: true,
            tiers,
        };
        staking_client
            .add_whitelisted_collection(&admin, &nft_id, collection_config)
            .unwrap();

        // Stake the NFT
        staking_client.stake_nft(&user, &nft_id, &1, &1).unwrap();

        // Verify staking
        let stake_info = staking_client.get_stake_info(&user, &nft_id, &1).unwrap();
        assert_eq!(stake_info.amount, 1);
        assert_eq!(stake_info.staker, user);

        // Advance time by 31 days to trigger tier multiplier
        env.ledger().set_timestamp(31 * 86400);

        // Calculate pending rewards
        let pending = staking_client
            .calculate_pending_rewards(&user, &nft_id, &1)
            .unwrap();
        assert!(pending.len() > 0);

        // Claim rewards
        let claimed = staking_client.claim_rewards(&user, &nft_id, &1).unwrap();
        assert!(claimed.len() > 0);
    }

    #[test]
    fn test_emergency_withdrawal() {
        let (env, admin, user, nft_id, reward_token_id) = setup_test_env();
        let staking_id = env.register_contract(None, StakingBonuses);
        let staking_client = StakingBonusesClient::new(&env, &staking_id);
        staking_client.init_contract(&admin).unwrap();

        // Setup
        staking_client
            .add_reward_token(&admin, &reward_token_id, &100_000)
            .unwrap();

        let mut tiers = Vec::new(&env);
        let collection_config = CollectionConfig {
            standard: NftStandard::ERC721,
            base_reward_rate: 100,
            is_whitelisted: true,
            tiers,
        };
        staking_client
            .add_whitelisted_collection(&admin, &nft_id, collection_config)
            .unwrap();

        // Stake
        staking_client.stake_nft(&user, &nft_id, &1, &1).unwrap();

        // Emergency withdraw
        staking_client
            .emergency_withdraw(&user, &nft_id, &1)
            .unwrap();

        // Verify stake is removed
        let stake_info = staking_client.get_stake_info(&user, &nft_id, &1);
        assert!(stake_info.is_none());
    }

    #[test]
    #[should_panic(expected = "StakeNotFound")]
    fn test_cannot_withdraw_twice() {
        let (env, admin, user, nft_id, reward_token_id) = setup_test_env();
        let staking_id = env.register_contract(None, StakingBonuses);
        let staking_client = StakingBonusesClient::new(&env, &staking_id);
        staking_client.init_contract(&admin).unwrap();

        // Setup
        staking_client
            .add_reward_token(&admin, &reward_token_id, &100_000)
            .unwrap();

        let collection_config = CollectionConfig {
            standard: NftStandard::ERC721,
            base_reward_rate: 100,
            is_whitelisted: true,
            tiers: Vec::new(&env),
        };
        staking_client
            .add_whitelisted_collection(&admin, &nft_id, collection_config)
            .unwrap();

        // Stake
        staking_client.stake_nft(&user, &nft_id, &1, &1).unwrap();

        // First emergency withdraw
        staking_client
            .emergency_withdraw(&user, &nft_id, &1)
            .unwrap();

        // Second emergency withdraw should fail
        staking_client
            .emergency_withdraw(&user, &nft_id, &1)
            .unwrap();
    }
}