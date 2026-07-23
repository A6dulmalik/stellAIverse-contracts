use soroban_sdk::{Env, Vec, Address};
use crate::types::{PriceEntry, AggregatedPrice};
use crate::errors::*;

pub struct PriceAggregator;

impl PriceAggregator {
    /// Aggregate prices from multiple providers using median aggregation
    pub fn aggregate_prices(
        env: &Env,
        prices: &Vec<PriceEntry>,
        min_sources: u32,
        max_staleness: u64,
    ) -> AggregatedPrice {
        if prices.len() < min_sources as usize {
            not_enough_sources();
        }

        // Filter out stale prices
        let current_time = env.ledger().timestamp();
        let mut valid_prices = Vec::new(&env);
        let mut is_fresh = true;

        for price in prices.iter() {
            if current_time - price.timestamp <= max_staleness {
                valid_prices.push_back(price.price);
            } else {
                is_fresh = false;
            }
        }

        if valid_prices.len() < min_sources as usize {
            not_enough_sources();
        }

        // Sort prices for median calculation
        let mut sorted_prices = valid_prices;
        sorted_prices.sort();

        // Calculate statistics
        let min_price = sorted_prices.get(0).unwrap();
        let max_price = sorted_prices.get(sorted_prices.len() - 1).unwrap();
        
        let median_idx = sorted_prices.len() / 2;
        let median_price = if sorted_prices.len() % 2 == 0 {
            // Average of two middle values for even length
            let left = sorted_prices.get(median_idx - 1).unwrap();
            let right = sorted_prices.get(median_idx).unwrap();
            (left + right) / 2
        } else {
            sorted_prices.get(median_idx).unwrap()
        };

        // Get the most recent timestamp from valid prices
        let mut latest_timestamp: u64 = 0;
        for entry in prices.iter() {
            if entry.timestamp > latest_timestamp {
                latest_timestamp = entry.timestamp;
            }
        }

        AggregatedPrice {
            price: median_price,
            timestamp: latest_timestamp,
            sources_used: valid_prices.len() as u32,
            min_price,
            max_price,
            median_price,
            is_fresh,
        }
    }

    /// Calculate weighted average based on provider reputation
    pub fn weighted_aggregate(
        env: &Env,
        prices: &Vec<(PriceEntry, u32)>, // (price entry, reputation score)
        min_sources: u32,
        max_staleness: u64,
    ) -> AggregatedPrice {
        if prices.len() < min_sources as usize {
            not_enough_sources();
        }

        let current_time = env.ledger().timestamp();
        let mut total_weight: u128 = 0;
        let mut weighted_sum: i128 = 0;
        let mut valid_count = 0;
        let mut is_fresh = true;
        let mut min_price = i128::MAX;
        let mut max_price = i128::MIN;
        let mut latest_timestamp: u64 = 0;

        for (entry, reputation) in prices.iter() {
            if current_time - entry.timestamp <= max_staleness {
                let weight = *reputation as u128;
                total_weight += weight;
                weighted_sum += entry.price * (weight as i128);
                valid_count += 1;

                if entry.price < min_price {
                    min_price = entry.price;
                }
                if entry.price > max_price {
                    max_price = entry.price;
                }
                if entry.timestamp > latest_timestamp {
                    latest_timestamp = entry.timestamp;
                }
            } else {
                is_fresh = false;
            }
        }

        if valid_count < min_sources {
            not_enough_sources();
        }

        let final_price = weighted_sum / (total_weight as i128);

        AggregatedPrice {
            price: final_price,
            timestamp: latest_timestamp,
            sources_used: valid_count as u32,
            min_price,
            max_price,
            median_price: final_price,
            is_fresh,
        }
    }

    /// Detect outliers using IQR method
    pub fn remove_outliers(env: &Env, mut prices: Vec<PriceEntry>) -> Vec<PriceEntry> {
        if prices.len() < 4 {
            return prices; // Not enough data to filter outliers
        }

        // Extract prices for IQR calculation
        let mut price_values = Vec::new(&env);
        for entry in prices.iter() {
            price_values.push_back(entry.price);
        }
        price_values.sort();

        let len = price_values.len();
        let q1_idx = len / 4;
        let q3_idx = (3 * len) / 4;
        let q1 = price_values.get(q1_idx).unwrap();
        let q3 = price_values.get(q3_idx).unwrap();
        let iqr = q3 - q1;
        let lower_bound = q1 - (i128::from(150) * iqr) / i128::from(100); // 1.5 * IQR
        let upper_bound = q3 + (i128::from(150) * iqr) / i128::from(100);

        // Filter out outliers
        let mut filtered = Vec::new(&env);
        for entry in prices.iter() {
            if entry.price >= lower_bound && entry.price <= upper_bound {
                filtered.push_back(entry);
            }
        }

        filtered
    }

    /// Validate price divergence between providers
    pub fn validate_price_divergence(
        prices: &Vec<PriceEntry>,
        max_divergence_bps: u32,
    ) -> bool {
        if prices.len() < 2 {
            return true;
        }

        let mut min_price = i128::MAX;
        let mut max_price = i128::MIN;

        for entry in prices.iter() {
            if entry.price < min_price {
                min_price = entry.price;
            }
            if entry.price > max_price {
                max_price = entry.price;
            }
        }

        // Calculate percentage difference
        if min_price == 0 {
            return false;
        }
        let divergence = ((max_price - min_price) * 10000) / min_price;
        divergence <= max_divergence_bps as i128
    }
}