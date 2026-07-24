#![no_std]
extern crate alloc;

pub mod contract;
pub mod errors;
pub mod storage_keys;
pub mod types;
pub mod utils;
#[cfg(test)]
mod test;

pub use contract::GovernanceContract;
pub use errors::GovernanceError;
pub use types::*;
pub use utils::*;

// Re-export the contract types for external use
#[allow(unused_imports)]
use soroban_sdk::contract;