#![allow(unused_imports)]
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    Unauthorized = 2,
    InvalidAmount = 3,
    StakeNotFound = 4,
    StakeLocked = 5,
    OverflowError = 6,
    CollectionNotWhitelisted = 7,
    NftAlreadyStaked = 8,
    CooldownNotElapsed = 9,
    InsufficientRewardBalance = 10,
    InvalidTierConfig = 11,
    RewardTokenNotSupported = 12,
    EmergencyWithdrawalAlreadyUsed = 13,
    InvalidNftStandard = 14,
    NftTransferFailed = 15,
    RewardCalculationFailed = 16,
}
