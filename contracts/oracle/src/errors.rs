use soroban_sdk::{panic_with_error, Symbol};

// Oracle error codes
#[inline(always)]
pub fn already_initialized() -> ! {
    panic_with_error!(&Symbol::new("already_init"))
}

#[inline(always)]
pub fn unauthorized() -> ! {
    panic_with_error!(&Symbol::new("unauthorized"))
}

#[inline(always)]
pub fn provider_not_found() -> ! {
    panic_with_error!(&Symbol::new("provider_not_found"))
}

#[inline(always)]
pub fn provider_already_exists() -> ! {
    panic_with_error!(&Symbol::new("provider_exists"))
}

#[inline(always)]
pub fn feed_not_found() -> ! {
    panic_with_error!(&Symbol::new("feed_not_found"))
}

#[inline(always)]
pub fn feed_already_exists() -> ! {
    panic_with_error!(&Symbol::new("feed_exists"))
}

#[inline(always)]
pub fn stale_price() -> ! {
    panic_with_error!(&Symbol::new("stale_price"))
}

#[inline(always)]
pub fn circuit_breaker_triggered() -> ! {
    panic_with_error!(&Symbol::new("circuit_breaker"))
}

#[inline(always)]
pub fn rate_limit_exceeded() -> ! {
    panic_with_error!(&Symbol::new("rate_limit"))
}

#[inline(always)]
pub fn insufficient_stake() -> ! {
    panic_with_error!(&Symbol::new("insufficient_stake"))
}

#[inline(always)]
pub fn invalid_price() -> ! {
    panic_with_error!(&Symbol::new("invalid_price"))
}

#[inline(always)]
pub fn update_too_early() -> ! {
    panic_with_error!(&Symbol::new("update_early"))
}

#[inline(always)]
pub fn no_active_providers() -> ! {
    panic_with_error!(&Symbol::new("no_providers"))
}

#[inline(always)]
pub fn aggregation_failed() -> ! {
    panic_with_error!(&Symbol::new("agg_fail"))
}

#[inline(always)]
pub fn subscription_expired() -> ! {
    panic_with_error!(&Symbol::new("sub_expired"))
}

#[inline(always)]
pub fn invalid_input() -> ! {
    panic_with_error!(&Symbol::new("invalid_input"))
}

#[inline(always)]
pub fn not_enough_sources() -> ! {
    panic_with_error!(&Symbol::new("not_enough_sources"))
}

#[inline(always)]
pub fn provider_inactive() -> ! {
    panic_with_error!(&Symbol::new("provider_inactive"))
}

#[inline(always)]
pub fn feed_inactive() -> ! {
    panic_with_error!(&Symbol::new("feed_inactive"))
}

#[inline(always)]
pub fn cooldown_active() -> ! {
    panic_with_error!(&Symbol::new("cooldown"))
}

#[inline(always)]
pub fn insufficient_balance() -> ! {
    panic_with_error!(&Symbol::new("insufficient_balance"))
}