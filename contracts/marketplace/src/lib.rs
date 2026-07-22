#![no_std]

use soroban_sdk::{
    contract, contractimpl, symbol_short, Address, Bytes, Env, IntoVal, String, Symbol, Val, Vec,
};

use stellai_lib::{WorkflowStep, WorkflowStepStatus};

// ── Storage keys ──────────────────────────────────────────────────────────────

const ADMIN_KEY: &str = "mkt_admin";
const LISTING_CTR_KEY: &str = "lst_ctr";
const LISTING_PREFIX: &str = "lst_";
const ROYALTY_PREFIX: &str = "roy_";
const AGENT_NFT_KEY: &str = "agent_nft";
const HUB_KEY: &str = "exec_hub";
const PENDING_SALE_PREFIX: &str = "psale_";
const WF_LISTING_PREFIX: &str = "wf_lst_";
// New storage keys for extended features
const AUCTION_CTR_KEY: &str = "auc_ctr";
const AUCTION_PREFIX: &str = "auc_";
const BID_RECORD_PREFIX: &str = "bid_";
const OFFER_CTR_KEY: &str = "ofr_ctr";
const OFFER_PREFIX: &str = "ofr_";
const DISPUTE_CTR_KEY: &str = "dsp_ctr";
const DISPUTE_PREFIX: &str = "dsp_";
const TRANSACTION_HISTORY_PREFIX: &str = "txn_";
const PLATFORM_FEE_KEY: &str = "plat_fee";
const DEFAULT_LISTING_DURATION: u64 = 30 * 24 * 60 * 60; // 30 days in seconds
const MIN_BID_INCREMENT_BPS: u32 = 100; // 1% minimum bid increment

// ── Local types ───────────────────────────────────────────────────────────────

#[derive(Clone)]
#[soroban_sdk::contracttype]
pub struct PendingSale {
    pub listing_id: u64,
    pub buyer: Address,
    pub amount: i128,
    pub seller: Address,
    pub agent_id: u64,
    pub workflow_id: u64,
    pub created_at: u64,
}

#[derive(Clone)]
#[soroban_sdk::contracttype]
pub struct Offer {
    pub offer_id: u64,
    pub listing_id: u64,
    pub offerer: Address,
    pub amount: i128,
    pub active: bool,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Clone)]
#[soroban_sdk::contracttype]
pub struct TransactionRecord {
    pub txn_id: u64,
    pub listing_id: u64,
    pub asset_id: u64,
    pub seller: Address,
    pub buyer: Address,
    pub amount: i128,
    pub royalty_amount: i128,
    pub platform_fee: i128,
    pub timestamp: u64,
    pub txn_type: String, // "sale", "auction_won", "offer_accepted"
}

#[derive(Clone)]
#[soroban_sdk::contracttype]
pub struct PlatformFeeConfig {
    pub fee_bps: u32,
    pub recipient: Address,
    pub min_fee: Option<i128>,
    pub max_fee: Option<i128>,
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct Marketplace;

#[contractimpl]
impl Marketplace {
    // =========================================================================
    // Initialisation
    // =========================================================================

    pub fn init_contract(env: Env, admin: Address) {
        let key = Symbol::new(&env, ADMIN_KEY);
        if env.storage().instance().has(&key) {
            panic!("Already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&key, &admin);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, LISTING_CTR_KEY), &0u64);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, AUCTION_CTR_KEY), &0u64);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, OFFER_CTR_KEY), &0u64);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, DISPUTE_CTR_KEY), &0u64);
        // Initialize default platform fee: 2.5%
        let default_fee = PlatformFeeConfig {
            fee_bps: 250,
            recipient: admin.clone(),
            min_fee: None,
            max_fee: None,
        };
        env.storage().instance().set(&Symbol::new(&env, PLATFORM_FEE_KEY), &default_fee);
    }

    pub fn set_agent_nft_contract(env: Env, admin: Address, agent_nft: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, AGENT_NFT_KEY), &agent_nft);
        env.events().publish((symbol_short!("nft_set"),), agent_nft);
    }

    pub fn set_execution_hub(env: Env, admin: Address, hub: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, HUB_KEY), &hub);
        env.events().publish((symbol_short!("hub_set"),), hub);
    }

    // =========================================================================
    // Listings
    // =========================================================================

    pub fn create_listing(
        env: Env,
        agent_id: u64,
        seller: Address,
        listing_type: u32,
        price: i128,
        duration_days: Option<u64>,
    ) -> u64 {
        seller.require_auth();
        if agent_id == 0 {
            panic!("Invalid agent ID");
        }
        if listing_type > 2 {
            panic!("Invalid listing type");
        }
        if !(stellai_lib::PRICE_LOWER_BOUND..=stellai_lib::PRICE_UPPER_BOUND).contains(&price) {
            panic!("Price out of valid range");
        }
        if listing_type == 1 {
            let dur = duration_days.expect("Duration required for leases");
            if dur == 0 || dur > stellai_lib::MAX_DURATION_DAYS {
                panic!("Lease duration out of valid range");
            }
        }

        let agent = Self::load_agent(&env, agent_id);
        if agent.owner != seller {
            panic!("Only agent owner can create listings");
        }
        if agent.escrow_locked {
            panic!("Agent already locked in escrow");
        }

        let listing_id = Self::next_listing_id(&env);
        let marketplace = env.current_contract_address();

        // Calculate expiration time
        let current_time = env.ledger().timestamp();
        let expires_at = if let Some(days) = duration_days {
            current_time + (days * 24 * 60 * 60)
        } else {
            current_time + DEFAULT_LISTING_DURATION
        };

        let listing_type_enum = match listing_type {
            0 => stellai_lib::ListingType::Sale,
            1 => stellai_lib::ListingType::Lease,
            2 => stellai_lib::ListingType::Auction,
            _ => panic!("Invalid listing type"),
        };

        let listing = stellai_lib::Listing {
            listing_id,
            asset_id: agent_id,
            asset_type: stellai_lib::AssetType::Agent,
            seller: seller.clone(),
            price,
            listing_type: listing_type_enum,
            active: true,
            created_at: current_time,
            expires_at,
        };

        let lk = Self::listing_key(&env, listing_id);
        env.storage().instance().set(&lk, &listing);

        let mut updated_agent = agent;
        updated_agent.escrow_locked = true;
        updated_agent.escrow_holder = Some(marketplace.clone());
        updated_agent.updated_at = env.ledger().timestamp();
        Self::save_agent(&env, agent_id, &updated_agent);

        env.events().publish(
            (symbol_short!("lst_creat"),),
            (listing_id, agent_id, seller.clone(), price),
        );
        env.events().publish(
            (symbol_short!("esc_lock"),),
            (agent_id, seller, marketplace),
        );

        listing_id
    }

    // =========================================================================
    // Execution-hub-orchestrated sale
    // =========================================================================

    /// Purchase an agent via an execution-hub workflow.
    ///
    /// Registers a three-step workflow in the hub, stores a pending-sale
    /// record, then drives step 0 immediately.  Remaining steps are driven by
    /// subsequent `execute_workflow_step` calls on the hub.
    ///
    /// Returns `(listing_id, workflow_id)`.
    pub fn buy_agent(env: Env, listing_id: u64, buyer: Address, amount: i128) -> (u64, u64) {
        buyer.require_auth();

        if listing_id == 0 {
            panic!("Invalid listing ID");
        }

        let listing = Self::load_listing(&env, listing_id);
        if !listing.active {
            panic!("Listing is not active");
        }
        if amount < listing.price {
            panic!("Insufficient payment");
        }
        if amount > stellai_lib::PRICE_UPPER_BOUND {
            panic!("Payment exceeds safe maximum");
        }

        let marketplace = env.current_contract_address();
        let agent = Self::load_agent(&env, listing.asset_id);
        if !agent.escrow_locked {
            panic!("Agent not in escrow");
        }
        match &agent.escrow_holder {
            Some(h) if h == &marketplace => {}
            _ => panic!("Agent locked by a different contract"),
        }

        // Persist pending sale (workflow_id filled in after the hub call)
        let pending = PendingSale {
            listing_id,
            buyer: buyer.clone(),
            amount,
            seller: listing.seller.clone(),
            agent_id: listing.asset_id,
            workflow_id: 0,
            created_at: env.ledger().timestamp(),
        };
        let psk = Self::pending_sale_key(&env, listing_id);
        env.storage().instance().set(&psk, &pending);

        let hub = Self::get_hub(&env);
        let steps = Self::build_sale_steps(&env, &marketplace, listing_id);
        let context_tag: Option<String> = Some(String::from_str(&env, "agent_sale"));
        let none_u64: Option<u64> = None;
        let cb_contract: Option<Address> = Some(marketplace.clone());

        // Build args for create_workflow
        let mut cw_args = Vec::<Val>::new(&env);
        cw_args.push_back(marketplace.clone().into_val(&env));
        cw_args.push_back(String::from_str(&env, "agent_sale").into_val(&env));
        cw_args.push_back(steps.into_val(&env));
        cw_args.push_back(none_u64.into_val(&env));
        cw_args.push_back(context_tag.into_val(&env));
        cw_args.push_back(cb_contract.into_val(&env));

        let workflow_id: u64 =
            env.invoke_contract(&hub, &Symbol::new(&env, "create_workflow"), cw_args);

        // Back-fill workflow_id
        let mut updated_pending: PendingSale = env
            .storage()
            .instance()
            .get(&psk)
            .expect("Pending sale disappeared");
        updated_pending.workflow_id = workflow_id;
        env.storage().instance().set(&psk, &updated_pending);

        // Store workflow→listing mapping for callback reconciliation
        let wlk = Self::wf_listing_key(&env, workflow_id);
        env.storage().instance().set(&wlk, &listing_id);

        env.events().publish(
            (symbol_short!("sale_init"),),
            (listing_id, buyer, workflow_id, env.ledger().timestamp()),
        );

        // Drive step 0
        let mut ews_args = Vec::<Val>::new(&env);
        ews_args.push_back(workflow_id.into_val(&env));
        let _: WorkflowStepStatus =
            env.invoke_contract(&hub, &Symbol::new(&env, "execute_workflow_step"), ews_args);

        (listing_id, workflow_id)
    }

    // =========================================================================
    // Workflow step functions (called by the execution hub)
    // =========================================================================

    /// Step 0 — verify the listing and escrow are still valid.
    /// `encoded_args`: 8 bytes big-endian listing_id.
    pub fn verify_sale(env: Env, encoded_args: Bytes) {
        let listing_id = Self::decode_u64(&encoded_args);
        let listing = Self::load_listing(&env, listing_id);
        if !listing.active {
            panic!("Listing no longer active");
        }
        let psk = Self::pending_sale_key(&env, listing_id);
        if !env.storage().instance().has(&psk) {
            panic!("No pending sale for this listing");
        }
        let marketplace = env.current_contract_address();
        let agent = Self::load_agent(&env, listing.asset_id);
        if !agent.escrow_locked {
            panic!("Agent not in escrow at verify time");
        }
        match &agent.escrow_holder {
            Some(h) if h == &marketplace => {}
            _ => panic!("Escrow holder mismatch at verify time"),
        }
        env.events().publish(
            (symbol_short!("sale_vfy"),),
            (listing_id, env.ledger().timestamp()),
        );
    }

    /// Step 1 — transfer ownership to the buyer.
    /// `encoded_args`: 8 bytes big-endian listing_id.
    pub fn transfer_ownership(env: Env, encoded_args: Bytes) {
        let listing_id = Self::decode_u64(&encoded_args);
        let listing = Self::load_listing(&env, listing_id);

        let psk = Self::pending_sale_key(&env, listing_id);
        let pending: PendingSale = env.storage().instance().get(&psk).expect("No pending sale");

        let mut agent = Self::load_agent(&env, listing.asset_id);
        agent.owner = pending.buyer.clone();
        agent.nonce = agent.nonce.checked_add(1).expect("Agent nonce overflow");
        agent.updated_at = env.ledger().timestamp();
        Self::save_agent(&env, listing.asset_id, &agent);

        env.events().publish(
            (symbol_short!("own_xfer"),),
            (
                listing.asset_id,
                listing.seller,
                pending.buyer,
                env.ledger().timestamp(),
            ),
        );
    }

    /// Step 2 — release escrow, deactivate listing, emit sale record.
    /// `encoded_args`: 8 bytes big-endian listing_id.
    pub fn record_sale(env: Env, encoded_args: Bytes) {
        let listing_id = Self::decode_u64(&encoded_args);
        let mut listing = Self::load_listing(&env, listing_id);

        let psk = Self::pending_sale_key(&env, listing_id);
        let pending: PendingSale = env.storage().instance().get(&psk).expect("No pending sale");

        let royalty_key = Self::royalty_key(&env, listing.asset_id);
        let royalty_info: Option<stellai_lib::RoyaltyInfo> =
            env.storage().instance().get(&royalty_key);

        let royalty_amount: i128 = if let Some(ref r) = royalty_info {
            if r.fee > stellai_lib::MAX_ROYALTY_PERCENTAGE {
                panic!("Invalid royalty percentage");
            }
            pending
                .amount
                .checked_mul(r.fee as i128)
                .expect("Royalty overflow")
                .checked_div(10_000)
                .expect("Royalty division")
        } else {
            0
        };

        let seller_amount = pending
            .amount
            .checked_sub(royalty_amount)
            .expect("Seller amount underflow");

        let mut agent = Self::load_agent(&env, listing.asset_id);
        agent.escrow_locked = false;
        agent.escrow_holder = None;
        agent.updated_at = env.ledger().timestamp();
        Self::save_agent(&env, listing.asset_id, &agent);

        listing.active = false;
        let lk = Self::listing_key(&env, listing_id);
        env.storage().instance().set(&lk, &listing);

        env.storage().instance().remove(&psk);

        env.events().publish(
            (symbol_short!("agnt_sold"),),
            (
                listing_id,
                listing.asset_id,
                pending.buyer.clone(),
                seller_amount,
                royalty_amount,
            ),
        );
        env.events().publish(
            (symbol_short!("esc_rel"),),
            (
                listing.asset_id,
                pending.buyer,
                env.current_contract_address(),
            ),
        );
    }

    // =========================================================================
    // Rollback (called by hub on failure)
    // =========================================================================

    /// Compensating action for the sale steps.
    /// Restores agent ownership to seller and releases escrow if needed.
    /// `encoded_args`: 8 bytes big-endian listing_id.
    pub fn rollback(env: Env, encoded_args: Bytes) {
        if encoded_args.is_empty() {
            return;
        }
        let listing_id = Self::decode_u64(&encoded_args);
        let psk = Self::pending_sale_key(&env, listing_id);
        let pending_opt: Option<PendingSale> = env.storage().instance().get(&psk);

        let pending = match pending_opt {
            Some(p) => p,
            None => return, // nothing to roll back
        };

        let listing_opt = Self::try_load_listing(&env, listing_id);
        if let Ok(listing) = listing_opt {
            if let Ok(mut agent) = Self::try_load_agent(&env, listing.asset_id) {
                // Restore ownership if it was transferred
                if agent.owner == pending.buyer {
                    agent.owner = pending.seller.clone();
                    agent.nonce = agent.nonce.checked_add(1).expect("Nonce overflow");
                    agent.updated_at = env.ledger().timestamp();
                    env.events().publish(
                        (symbol_short!("rb_own"),),
                        (
                            listing.asset_id,
                            pending.buyer.clone(),
                            pending.seller.clone(),
                            env.ledger().timestamp(),
                        ),
                    );
                }
                // Release escrow
                if agent.escrow_locked {
                    agent.escrow_locked = false;
                    agent.escrow_holder = None;
                    agent.updated_at = env.ledger().timestamp();
                    env.events().publish(
                        (symbol_short!("rb_esc"),),
                        (listing.asset_id, env.ledger().timestamp()),
                    );
                }
                Self::save_agent(&env, listing.asset_id, &agent);
            }
        }

        env.storage().instance().remove(&psk);
    }

    // =========================================================================
    // Standard execution-hub step interface
    // =========================================================================

    /// Entry point called by the execution hub for every workflow step.
    /// Dispatches to the correct step function based on step_index.
    pub fn exec_step(env: Env, step_index: u32, encoded_args: Bytes) {
        match step_index {
            0 => Self::verify_sale(env, encoded_args),
            1 => Self::transfer_ownership(env, encoded_args),
            2 => Self::record_sale(env, encoded_args),
            _ => panic!("Unknown step index"),
        }
    }

    // =========================================================================
    // Workflow completion callback (called by hub)
    // =========================================================================

    /// `status`: 2=Completed, 3=RolledBack, 4=Failed, 5=Cancelled
    pub fn wf_done(env: Env, workflow_id: u64, status: u32) {
        let wlk = Self::wf_listing_key(&env, workflow_id);
        let listing_id: Option<u64> = env.storage().instance().get(&wlk);

        let lid = match listing_id {
            Some(id) => id,
            None => return,
        };

        let psk = Self::pending_sale_key(&env, lid);

        match status {
            2 => {
                // Completed — remove cross-reference
                env.storage().instance().remove(&wlk);
                env.events().publish(
                    (symbol_short!("cb_ok"),),
                    (workflow_id, lid, env.ledger().timestamp()),
                );
            }
            3..=5 => {
                // RolledBack / Failed / Cancelled — ensure listing stays active
                if let Ok(mut listing) = Self::try_load_listing(&env, lid) {
                    if !listing.active {
                        listing.active = true;
                        let lk = Self::listing_key(&env, lid);
                        env.storage().instance().set(&lk, &listing);
                    }
                }
                if env.storage().instance().has(&psk) {
                    env.storage().instance().remove(&psk);
                }
                env.storage().instance().remove(&wlk);
                env.events().publish(
                    (symbol_short!("cb_fail"),),
                    (workflow_id, lid, status, env.ledger().timestamp()),
                );
            }
            _ => {}
        }
    }

    // =========================================================================
    // Auto-expire listings
    // =========================================================================
    
    /// Check and expire any listings that have passed their expiration date
    pub fn cleanup_expired_listings(env: Env, listing_ids: Vec<u64>) -> Vec<u64> {
        let current_time = env.ledger().timestamp();
        let mut expired_listings = Vec::new(&env);
        let marketplace = env.current_contract_address();
        
        for i in 0..listing_ids.len() {
            if let Some(listing_id) = listing_ids.get(i) {
                if let Ok(mut listing) = Self::try_load_listing(&env, listing_id) {
                    if listing.active && listing.expires_at < current_time {
                        // Auto-delist the expired listing
                        listing.active = false;
                        let lk = Self::listing_key(&env, listing_id);
                        env.storage().instance().set(&lk, &listing);
                        
                        // Release escrow
                        let mut agent = Self::load_agent(&env, listing.asset_id);
                        if agent.escrow_locked {
                            match &agent.escrow_holder {
                                Some(h) if h == &marketplace => {
                                    agent.escrow_locked = false;
                                    agent.escrow_holder = None;
                                    agent.updated_at = current_time;
                                    agent.nonce = agent.nonce.checked_add(1).expect("Nonce overflow");
                                    Self::save_agent(&env, listing.asset_id, &agent);
                                }
                                _ => {}
                            }
                        }
                        
                        expired_listings.push_back(listing_id);
                        env.events().publish(
                            (symbol_short!("lst_exp"),),
                            (listing_id, listing.asset_id, current_time),
                        );
                    }
                }
            }
        }
        expired_listings
    }

    // =========================================================================
    // Cancel listing
    // =========================================================================

    pub fn cancel_listing(env: Env, listing_id: u64, seller: Address) {
        seller.require_auth();
        if listing_id == 0 {
            panic!("Invalid listing ID");
        }
        let mut listing = Self::load_listing(&env, listing_id);
        if listing.seller != seller {
            panic!("Only seller can cancel listing");
        }
        if !listing.active {
            panic!("Listing is not active");
        }

        let marketplace = env.current_contract_address();
        let mut agent = Self::load_agent(&env, listing.asset_id);
        if agent.escrow_locked {
            match &agent.escrow_holder {
                Some(h) if h == &marketplace => {
                    agent.escrow_locked = false;
                    agent.escrow_holder = None;
                    agent.updated_at = env.ledger().timestamp();
                    agent.nonce = agent.nonce.checked_add(1).expect("Nonce overflow");
                    Self::save_agent(&env, listing.asset_id, &agent);
                }
                _ => panic!("Agent locked by a different contract"),
            }
        }

        listing.active = false;
        let lk = Self::listing_key(&env, listing_id);
        env.storage().instance().set(&lk, &listing);

        env.events().publish(
            (symbol_short!("lst_cncl"),),
            (listing_id, listing.asset_id, seller),
        );
    }

    // =========================================================================
    // Offer and Counter-offer System
    // =========================================================================

    /// Create an offer on an active listing
    pub fn make_offer(
        env: Env,
        listing_id: u64,
        offerer: Address,
        amount: i128,
        duration_days: Option<u64>,
    ) -> u64 {
        offerer.require_auth();
        
        if listing_id == 0 {
            panic!("Invalid listing ID");
        }
        if amount <= 0 || amount > stellai_lib::PRICE_UPPER_BOUND {
            panic!("Invalid offer amount");
        }

        let listing = Self::load_listing(&env, listing_id);
        if !listing.active {
            panic!("Listing is not active");
        }
        if listing.expires_at < env.ledger().timestamp() {
            panic!("Listing has expired");
        }

        let offer_id = Self::next_offer_id(&env);
        let current_time = env.ledger().timestamp();
        let expires_at = if let Some(days) = duration_days {
            current_time + (days * 24 * 60 * 60)
        } else {
            current_time + 7 * 24 * 60 * 60 // 7 days default
        };

        let offer = Offer {
            offer_id,
            listing_id,
            offerer: offerer.clone(),
            amount,
            active: true,
            created_at: current_time,
            expires_at,
        };

        let ok = Self::offer_key(&env, offer_id);
        env.storage().instance().set(&ok, &offer);

        env.events().publish(
            (symbol_short!("ofr_made"),),
            (offer_id, listing_id, offerer, amount, expires_at),
        );

        offer_id
    }

    /// Accept an offer (only seller can accept)
    pub fn accept_offer(env: Env, offer_id: u64, seller: Address) -> (u64, u64) {
        seller.require_auth();
        
        if offer_id == 0 {
            panic!("Invalid offer ID");
        }

        let mut offer: Offer = env.storage()
            .instance()
            .get(&Self::offer_key(&env, offer_id))
            .expect("Offer not found");

        if !offer.active {
            panic!("Offer is not active");
        }
        if offer.expires_at < env.ledger().timestamp() {
            panic!("Offer has expired");
        }

        let listing = Self::load_listing(&env, offer.listing_id);
        if listing.seller != seller {
            panic!("Only listing seller can accept offers");
        }
        if !listing.active {
            panic!("Listing is no longer active");
        }

        // Mark offer as inactive
        offer.active = false;
        env.storage().instance().set(&Self::offer_key(&env, offer_id), &offer);

        // Start the purchase workflow
        Self::buy_agent(env, offer.listing_id, offer.offerer, offer.amount)
    }

    /// Reject an offer
    pub fn reject_offer(env: Env, offer_id: u64, caller: Address) {
        caller.require_auth();
        
        let mut offer: Offer = env.storage()
            .instance()
            .get(&Self::offer_key(&env, offer_id))
            .expect("Offer not found");

        let listing = Self::load_listing(&env, offer.listing_id);
        if listing.seller != caller && offer.offerer != caller {
            panic!("Only involved parties can reject offers");
        }

        if offer.active {
            offer.active = false;
            env.storage().instance().set(&Self::offer_key(&env, offer_id), &offer);
            env.events().publish(
                (symbol_short!("ofr_rjct"),),
                (offer_id, caller, env.ledger().timestamp()),
            );
        }
    }

    // =========================================================================
    // Auction System
    // =========================================================================

    /// Create an English auction for an asset
    pub fn create_auction(
        env: Env,
        agent_id: u64,
        seller: Address,
        start_price: i128,
        reserve_price: i128,
        duration_days: u64,
        min_bid_increment_bps: Option<u32>,
    ) -> u64 {
        seller.require_auth();
        
        if agent_id == 0 {
            panic!("Invalid agent ID");
        }
        if start_price <= 0 || reserve_price <= 0 {
            panic!("Prices must be positive");
        }
        if reserve_price > start_price {
            panic!("Reserve price cannot exceed start price");
        }
        if duration_days == 0 || duration_days > 365 {
            panic!("Invalid auction duration");
        }

        let agent = Self::load_agent(&env, agent_id);
        if agent.owner != seller {
            panic!("Only owner can create auctions");
        }
        if agent.escrow_locked {
            panic!("Agent already locked in escrow");
        }

        let auction_id = Self::next_auction_id(&env);
        let current_time = env.ledger().timestamp();
        let end_time = current_time + (duration_days * 24 * 60 * 60);
        let min_increment = min_bid_increment_bps.unwrap_or(MIN_BID_INCREMENT_BPS);

        if min_increment < 10 || min_increment > 10000 {
            panic!("Invalid bid increment (must be 0.1% to 100%)");
        }

        let marketplace = env.current_contract_address();
        let mut updated_agent = agent;
        updated_agent.escrow_locked = true;
        updated_agent.escrow_holder = Some(marketplace.clone());
        updated_agent.updated_at = current_time;
        Self::save_agent(&env, agent_id, &updated_agent);

        let auction = stellai_lib::Auction {
            auction_id,
            agent_id,
            seller: seller.clone(),
            auction_type: stellai_lib::AuctionType::English,
            start_price,
            reserve_price,
            current_price: start_price,
            highest_bidder: None,
            highest_bid: 0,
            start_time: current_time,
            end_time,
            min_bid_increment_bps: min_increment,
            status: stellai_lib::AuctionStatus::Active,
            dutch_config: None,
            sealed_commit_end: None,
            sealed_reveal_end: None,
        };

        let ak = Self::auction_key(&env, auction_id);
        env.storage().instance().set(&ak, &auction);

        env.events().publish(
            (symbol_short!("auc_creat"),),
            (auction_id, agent_id, seller, start_price, end_time),
        );

        auction_id
    }

    /// Place a bid on an active auction
    pub fn place_bid(env: Env, auction_id: u64, bidder: Address, bid_amount: i128) {
        bidder.require_auth();
        
        if auction_id == 0 {
            panic!("Invalid auction ID");
        }
        if bid_amount <= 0 {
            panic!("Bid amount must be positive");
        }

        let mut auction: stellai_lib::Auction = env.storage()
            .instance()
            .get(&Self::auction_key(&env, auction_id))
            .expect("Auction not found");

        let current_time = env.ledger().timestamp();
        if auction.status != stellai_lib::AuctionStatus::Active {
            panic!("Auction is not active");
        }
        if current_time > auction.end_time {
            panic!("Auction has ended");
        }

        // Calculate minimum bid required
        let min_bid = if auction.highest_bid == 0 {
            auction.start_price
        } else {
            let min_increment = (auction.highest_bid * (auction.min_bid_increment_bps as i128)) / 10000;
            auction.highest_bid + min_increment
        };

        if bid_amount < min_bid {
            panic!("Bid too low - minimum required: {}", min_bid);
        }

        // Refund previous highest bidder if exists
        if let Some(prev_bidder) = auction.highest_bidder {
            env.events().publish(
                (symbol_short!("bid_refnd"),),
                (auction_id, prev_bidder, auction.highest_bid, current_time),
            );
        }

        // Record the new bid
        let bid_sequence = Self::record_bid(&env, auction_id, bidder.clone(), bid_amount, current_time);
        
        auction.highest_bidder = Some(bidder.clone());
        auction.highest_bid = bid_amount;
        auction.current_price = bid_amount;
        env.storage().instance().set(&Self::auction_key(&env, auction_id), &auction);

        env.events().publish(
            (symbol_short!("bid_plcd"),),
            (auction_id, bidder, bid_amount, bid_sequence, current_time),
        );
    }

    /// Finalize an auction after it has ended
    pub fn finalize_auction(env: Env, auction_id: u64) {
        if auction_id == 0 {
            panic!("Invalid auction ID");
        }

        let mut auction: stellai_lib::Auction = env.storage()
            .instance()
            .get(&Self::auction_key(&env, auction_id))
            .expect("Auction not found");

        let current_time = env.ledger().timestamp();
        if auction.status != stellai_lib::AuctionStatus::Active {
            panic!("Auction already processed");
        }
        if current_time <= auction.end_time {
            panic!("Auction has not ended yet");
        }

        // Check if reserve price was met
        if auction.highest_bid >= auction.reserve_price {
            // Auction was successful - highest bidder wins
            auction.status = stellai_lib::AuctionStatus::Won;
            
            if let Some(ref buyer) = auction.highest_bidder {
                // Process the sale - transfer ownership and distribute funds
                Self::process_auction_sale(&env, &auction, buyer.clone());
            }
            
            env.events().publish(
                (symbol_short!("auc_won"),),
                (auction_id, auction.highest_bidder.clone(), auction.highest_bid, current_time),
            );
        } else {
            // Reserve not met - cancel auction, return asset to seller
            auction.status = stellai_lib::AuctionStatus::Ended;
            Self::cancel_auction_asset_return(&env, &auction);
            
            env.events().publish(
                (symbol_short!("auc_exp"),),
                (auction_id, auction.reserve_price, auction.highest_bid, current_time),
            );
        }

        env.storage().instance().set(&Self::auction_key(&env, auction_id), &auction);
    }

    /// Cancel an auction and return the asset to the seller
    fn cancel_auction_asset_return(env: &Env, auction: &stellai_lib::Auction) {
        let marketplace = env.current_contract_address();
        let mut agent = Self::load_agent(env, auction.agent_id);
        
        if agent.escrow_locked {
            match &agent.escrow_holder {
                Some(h) if h == &marketplace => {
                    agent.escrow_locked = false;
                    agent.escrow_holder = None;
                    agent.updated_at = env.ledger().timestamp();
                    Self::save_agent(env, auction.agent_id, &agent);
                }
                _ => panic!("Agent locked by different contract"),
            }
        }
    }

    /// Process a successful auction sale
    fn process_auction_sale(env: &Env, auction: &stellai_lib::Auction, buyer: Address) {
        let mut agent = Self::load_agent(env, auction.agent_id);
        
        // Transfer ownership to the winning bidder
        agent.owner = buyer.clone();
        agent.escrow_locked = false;
        agent.escrow_holder = None;
        agent.updated_at = env.ledger().timestamp();
        agent.nonce = agent.nonce.checked_add(1).expect("Nonce overflow");
        Self::save_agent(env, auction.agent_id, &agent);

        // Calculate royalties and platform fees
        let royalty_key = Self::royalty_key(env, auction.agent_id);
        let royalty_info: Option<stellai_lib::RoyaltyInfo> = env.storage().instance().get(&royalty_key);
        let platform_fee_config: PlatformFeeConfig = env.storage()
            .instance()
            .get(&Symbol::new(env, PLATFORM_FEE_KEY))
            .expect("Platform fee not configured");

        let mut royalty_amount = 0;
        if let Some(r) = royalty_info {
            if r.fee <= stellai_lib::MAX_ROYALTY_PERCENTAGE {
                royalty_amount = (auction.highest_bid * (r.fee as i128)) / 10000;
            }
        }

        let platform_fee = (auction.highest_bid * (platform_fee_config.fee_bps as i128)) / 10000;
        let seller_amount = auction.highest_bid - royalty_amount - platform_fee;

        // Record transaction for history
        Self::record_transaction(
            env,
            0, // listing_id - 0 for auctions
            auction.agent_id,
            auction.seller.clone(),
            buyer.clone(),
            auction.highest_bid,
            royalty_amount,
            platform_fee,
            String::from_str(env, "auction_won"),
        );

        env.events().publish(
            (symbol_short!("auc_sold"),),
            (
                auction.auction_id,
                auction.agent_id,
                auction.seller.clone(),
                buyer,
                seller_amount,
                royalty_amount,
                platform_fee,
            ),
        );
    }

    /// Record a bid for historical tracking
    fn record_bid(env: &Env, auction_id: u64, bidder: Address, amount: i128, timestamp: u64) -> u64 {
        let bid_key = (String::from_str(env, BID_RECORD_PREFIX), auction_id);
        let bids: Vec<stellai_lib::BidRecord> = env.storage()
            .instance()
            .get(&bid_key)
            .unwrap_or_else(|| Vec::new(env));
        
        let sequence = (bids.len() as u64) + 1;
        let mut new_bids = bids.clone();
        new_bids.push_back(stellai_lib::BidRecord {
            bidder,
            amount,
            timestamp,
            bid_increment: if bids.len() > 0 {
                let prev_bid = bids.last().unwrap();
                amount - prev_bid.amount
            } else {
                0
            },
            sequence,
        });

        env.storage().instance().set(&bid_key, &new_bids);
        sequence
    }

    // =========================================================================
    // Dispute Resolution System
    // =========================================================================

    /// Open a dispute for a transaction
    pub fn open_dispute(
        env: Env,
        listing_id: u64,
        initiator: Address,
        reason: String,
        evidence_cid: Option<String>,
    ) -> u64 {
        initiator.require_auth();
        
        if listing_id == 0 {
            panic!("Invalid listing ID");
        }
        if reason.len() == 0 || reason.len() > 1024 {
            panic!("Invalid dispute reason length");
        }

        let dispute_id = Self::next_dispute_id(&env);
        let current_time = env.ledger().timestamp();

        let dispute = stellai_lib::Dispute {
            dispute_id,
            listing_id,
            asset_type: stellai_lib::AssetType::Agent,
            initiator: initiator.clone(),
            reason,
            evidence_cid,
            status: stellai_lib::DisputeStatus::Open,
            created_at: current_time,
            resolved_at: None,
        };

        let dk = Self::dispute_key(&env, dispute_id);
        env.storage().instance().set(&dk, &dispute);

        env.events().publish(
            (symbol_short!("dsp_open"),),
            (dispute_id, listing_id, initiator, current_time),
        );

        dispute_id
    }

    /// Admin resolves a dispute
    pub fn resolve_dispute(
        env: Env,
        dispute_id: u64,
        admin: Address,
        ruling: bool, // true = side with initiator, false = reject dispute
        resolution_notes: Option<String>,
    ) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        if dispute_id == 0 {
            panic!("Invalid dispute ID");
        }

        let mut dispute: stellai_lib::Dispute = env.storage()
            .instance()
            .get(&Self::dispute_key(&env, dispute_id))
            .expect("Dispute not found");

        if dispute.status != stellai_lib::DisputeStatus::Open {
            panic!("Dispute is already resolved");
        }

        let current_time = env.ledger().timestamp();
        dispute.resolved_at = Some(current_time);
        dispute.status = if ruling {
            stellai_lib::DisputeStatus::Resolved
        } else {
            stellai_lib::DisputeStatus::Rejected
        };

        env.storage().instance().set(&Self::dispute_key(&env, dispute_id), &dispute);

        env.events().publish(
            (symbol_short!("dsp_res"),),
            (dispute_id, ruling as u32, current_time, resolution_notes),
        );
    }

    /// Get all active disputes in the queue
    pub fn get_active_disputes(env: Env, dispute_ids: Vec<u64>) -> Vec<stellai_lib::Dispute> {
        let mut active_disputes = Vec::new(&env);
        let current_time = env.ledger().timestamp();

        for i in 0..dispute_ids.len() {
            if let Some(dispute_id) = dispute_ids.get(i) {
                if let Ok(dispute) = Self::try_load_dispute(&env, dispute_id) {
                    if dispute.status == stellai_lib::DisputeStatus::Open {
                        active_disputes.push_back(dispute);
                    }
                }
            }
        }
        active_disputes
    }

    // =========================================================================
    // Transaction History & Analytics
    // =========================================================================

    /// Record a transaction in the history
    fn record_transaction(
        env: &Env,
        listing_id: u64,
        asset_id: u64,
        seller: Address,
        buyer: Address,
        amount: i128,
        royalty_amount: i128,
        platform_fee: i128,
        txn_type: String,
    ) -> u64 {
        static mut TXN_CTR: u64 = 0;
        let key = Symbol::new(env, "txn_ctr");
        let current: u64 = env.storage().instance().get(&key).unwrap_or(0);
        let txn_id = current + 1;
        env.storage().instance().set(&key, &txn_id);

        let record = TransactionRecord {
            txn_id,
            listing_id,
            asset_id,
            seller,
            buyer,
            amount,
            royalty_amount,
            platform_fee,
            timestamp: env.ledger().timestamp(),
            txn_type,
        };

        let tk = Self::transaction_key(env, txn_id);
        env.storage().instance().set(&tk, &record);

        txn_id
    }

    /// Get transaction history for a user (buyer or seller)
    pub fn get_user_transactions(env: Env, user: Address, txn_ids: Vec<u64>) -> Vec<TransactionRecord> {
        let mut user_txns = Vec::new(&env);

        for i in 0..txn_ids.len() {
            if let Some(txn_id) = txn_ids.get(i) {
                if let Some(record) = env.storage().instance().get::<_, TransactionRecord>(&Self::transaction_key(&env, txn_id)) {
                    if record.seller == user || record.buyer == user {
                        user_txns.push_back(record);
                    }
                }
            }
        }
        user_txns
    }

    /// Get platform analytics (volume, fees, etc.) - admin only
    pub fn get_platform_analytics(env: Env, admin: Address, start_time: u64, end_time: u64) -> (i128, i128, u64) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let mut total_volume: i128 = 0;
        let mut total_fees: i128 = 0;
        let mut txn_count: u64 = 0;

        // This would typically iterate through a range of transactions
        // For simplicity, this is a placeholder for the analytics calculation

        (total_volume, total_fees, txn_count)
    }

    // =========================================================================
    // Admin Tools
    // =========================================================================

    /// Update platform fee configuration (admin only)
    pub fn set_platform_fee(env: Env, admin: Address, fee_bps: u32, recipient: Address) {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        if fee_bps > 1000 {
            panic!("Platform fee cannot exceed 10%");
        }

        let mut config: PlatformFeeConfig = env.storage()
            .instance()
            .get(&Symbol::new(&env, PLATFORM_FEE_KEY))
            .expect("Platform fee config not found");
        
        config.fee_bps = fee_bps;
        config.recipient = recipient.clone();
        
        env.storage().instance().set(&Symbol::new(&env, PLATFORM_FEE_KEY), &config);

        env.events().publish(
            (symbol_short!("fee_upd"),),
            (fee_bps, recipient, env.ledger().timestamp()),
        );
    }

    // =========================================================================
    // Royalties
    // =========================================================================

    pub fn set_royalty(
        env: Env,
        agent_id: u64,
        creator: Address,
        recipient: Address,
        percentage: u32,
    ) {
        creator.require_auth();
        if agent_id == 0 {
            panic!("Invalid agent ID");
        }
        if percentage > stellai_lib::MAX_ROYALTY_PERCENTAGE {
            panic!("Royalty exceeds maximum");
        }
        let agent = Self::load_agent(&env, agent_id);
        if agent.owner != creator {
            panic!("Only agent owner can set royalty");
        }
        let rk = Self::royalty_key(&env, agent_id);
        env.storage().instance().set(
            &rk,
            &stellai_lib::RoyaltyInfo {
                recipient,
                fee: percentage,
            },
        );
        env.events()
            .publish((symbol_short!("roy_set"),), (agent_id, percentage));
    }

    pub fn get_royalty(env: Env, agent_id: u64) -> Option<stellai_lib::RoyaltyInfo> {
        if agent_id == 0 {
            panic!("Invalid agent ID");
        }
        env.storage()
            .instance()
            .get(&Self::royalty_key(&env, agent_id))
    }

    // =========================================================================
    // Queries
    // =========================================================================

    pub fn get_listing(env: Env, listing_id: u64) -> stellai_lib::Listing {
        Self::load_listing(&env, listing_id)
    }

    pub fn get_pending_sale(env: Env, listing_id: u64) -> Option<PendingSale> {
        env.storage()
            .instance()
            .get(&Self::pending_sale_key(&env, listing_id))
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, ADMIN_KEY))
            .expect("Not initialized")
    }

    pub fn get_execution_hub(env: Env) -> Address {
        Self::get_hub(&env)
    }

    // =========================================================================
    // Private helpers
    // =========================================================================

    fn listing_key(env: &Env, listing_id: u64) -> (String, u64) {
        (String::from_str(env, LISTING_PREFIX), listing_id)
    }

    fn royalty_key(env: &Env, agent_id: u64) -> (String, u64) {
        (String::from_str(env, ROYALTY_PREFIX), agent_id)
    }

    fn pending_sale_key(env: &Env, listing_id: u64) -> (String, u64) {
        (String::from_str(env, PENDING_SALE_PREFIX), listing_id)
    }

    fn wf_listing_key(env: &Env, workflow_id: u64) -> (String, u64) {
        (String::from_str(env, WF_LISTING_PREFIX), workflow_id)
    }

    fn agent_key(env: &Env, agent_id: u64) -> (String, u64) {
        (
            String::from_str(env, stellai_lib::AGENT_KEY_PREFIX),
            agent_id,
        )
    }

    fn load_agent(env: &Env, agent_id: u64) -> stellai_lib::Agent {
        env.storage()
            .instance()
            .get(&Self::agent_key(env, agent_id))
            .expect("Agent not found")
    }

    fn try_load_agent(env: &Env, agent_id: u64) -> Result<stellai_lib::Agent, ()> {
        env.storage()
            .instance()
            .get(&Self::agent_key(env, agent_id))
            .ok_or(())
    }

    fn save_agent(env: &Env, agent_id: u64, agent: &stellai_lib::Agent) {
        env.storage()
            .instance()
            .set(&Self::agent_key(env, agent_id), agent);
    }

    fn load_listing(env: &Env, listing_id: u64) -> stellai_lib::Listing {
        env.storage()
            .instance()
            .get(&Self::listing_key(env, listing_id))
            .expect("Listing not found")
    }

    fn try_load_listing(env: &Env, listing_id: u64) -> Result<stellai_lib::Listing, ()> {
        env.storage()
            .instance()
            .get(&Self::listing_key(env, listing_id))
            .ok_or(())
    }

    fn get_hub(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&Symbol::new(env, HUB_KEY))
            .expect("Execution hub not set")
    }

    fn next_listing_id(env: &Env) -> u64 {
        let key = Symbol::new(env, LISTING_CTR_KEY);
        let current: u64 = env.storage().instance().get(&key).unwrap_or(0);
        let next = current.checked_add(1).expect("Listing ID overflow");
        env.storage().instance().set(&key, &next);
        next
    }

    fn next_auction_id(env: &Env) -> u64 {
        let key = Symbol::new(env, AUCTION_CTR_KEY);
        let current: u64 = env.storage().instance().get(&key).unwrap_or(0);
        let next = current.checked_add(1).expect("Auction ID overflow");
        env.storage().instance().set(&key, &next);
        next
    }

    fn next_offer_id(env: &Env) -> u64 {
        let key = Symbol::new(env, OFFER_CTR_KEY);
        let current: u64 = env.storage().instance().get(&key).unwrap_or(0);
        let next = current.checked_add(1).expect("Offer ID overflow");
        env.storage().instance().set(&key, &next);
        next
    }

    fn next_dispute_id(env: &Env) -> u64 {
        let key = Symbol::new(env, DISPUTE_CTR_KEY);
        let current: u64 = env.storage().instance().get(&key).unwrap_or(0);
        let next = current.checked_add(1).expect("Dispute ID overflow");
        env.storage().instance().set(&key, &next);
        next
    }

    fn auction_key(env: &Env, auction_id: u64) -> (String, u64) {
        (String::from_str(env, AUCTION_PREFIX), auction_id)
    }

    fn offer_key(env: &Env, offer_id: u64) -> (String, u64) {
        (String::from_str(env, OFFER_PREFIX), offer_id)
    }

    fn dispute_key(env: &Env, dispute_id: u64) -> (String, u64) {
        (String::from_str(env, DISPUTE_PREFIX), dispute_id)
    }

    fn transaction_key(env: &Env, txn_id: u64) -> (String, u64) {
        (String::from_str(env, TRANSACTION_HISTORY_PREFIX), txn_id)
    }

    fn try_load_dispute(env: &Env, dispute_id: u64) -> Result<stellai_lib::Dispute, ()> {
        env.storage()
            .instance()
            .get(&Self::dispute_key(env, dispute_id))
            .ok_or(())
    }

    fn assert_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(env, ADMIN_KEY))
            .expect("Not initialized");
        if caller != &admin {
            panic!("Unauthorized");
        }
    }

    fn build_sale_steps(env: &Env, marketplace: &Address, listing_id: u64) -> Vec<WorkflowStep> {
        let encoded = Self::encode_u64(env, listing_id);

        let step0 = WorkflowStep {
            step_index: 0,
            name: String::from_str(env, "verify_sale"),
            target_contract: marketplace.clone(),
            function_name: String::from_str(env, "verify_sale"),
            encoded_args: encoded.clone(),
            required: true,
            max_retries: 0,
            retry_count: 0,
            status: WorkflowStepStatus::Pending,
            result: None,
            error: None,
            updated_at: 0,
        };

        let step1 = WorkflowStep {
            step_index: 1,
            name: String::from_str(env, "transfer_ownership"),
            target_contract: marketplace.clone(),
            function_name: String::from_str(env, "transfer_ownership"),
            encoded_args: encoded.clone(),
            required: true,
            max_retries: 1,
            retry_count: 0,
            status: WorkflowStepStatus::Pending,
            result: None,
            error: None,
            updated_at: 0,
        };

        let step2 = WorkflowStep {
            step_index: 2,
            name: String::from_str(env, "record_sale"),
            target_contract: marketplace.clone(),
            function_name: String::from_str(env, "record_sale"),
            encoded_args: encoded,
            required: true,
            max_retries: 0,
            retry_count: 0,
            status: WorkflowStepStatus::Pending,
            result: None,
            error: None,
            updated_at: 0,
        };

        let mut steps = Vec::new(env);
        steps.push_back(step0);
        steps.push_back(step1);
        steps.push_back(step2);
        steps
    }

    fn encode_u64(env: &Env, value: u64) -> Bytes {
        Bytes::from_array(env, &value.to_be_bytes())
    }

    fn decode_u64(data: &Bytes) -> u64 {
        if data.len() < 8 {
            panic!("Encoded args too short");
        }
        let mut arr = [0u8; 8];
        for (i, byte) in arr.iter_mut().enumerate() {
            *byte = data.get(i as u32).expect("byte missing");
        }
        u64::from_be_bytes(arr)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    fn setup_marketplace(env: &Env) -> (Address, Address) {
        let contract_id = env.register(Marketplace, ());
        let admin = Address::generate(env);
        MarketplaceClient::new(env, &contract_id).init_contract(&admin);
        (contract_id, admin)
    }

    fn seed_agent(env: &Env, contract_id: &Address, agent_id: u64, owner: &Address) {
        env.as_contract(contract_id, || {
            let key = (
                String::from_str(env, stellai_lib::AGENT_KEY_PREFIX),
                agent_id,
            );
            env.storage().instance().set(
                &key,
                &stellai_lib::Agent {
                    id: agent_id,
                    owner: owner.clone(),
                    name: String::from_str(env, "Bot"),
                    model_hash: String::from_str(env, "h"),
                    metadata_cid: String::from_str(env, "c"),
                    capabilities: Vec::new(env),
                    evolution_level: 0,
                    created_at: 0,
                    updated_at: 0,
                    nonce: 0,
                    escrow_locked: false,
                    escrow_holder: None,
                },
            );
        });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Initialisation
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_init() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, admin) = setup_marketplace(&env);
        assert_eq!(
            MarketplaceClient::new(&env, &contract_id).get_admin(),
            admin
        );
    }

    #[test]
    #[should_panic(expected = "Already initialized")]
    fn test_double_init() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, admin) = setup_marketplace(&env);
        MarketplaceClient::new(&env, &contract_id).init_contract(&admin);
    }

    #[test]
    fn test_set_execution_hub() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, admin) = setup_marketplace(&env);
        let hub = Address::generate(&env);
        let client = MarketplaceClient::new(&env, &contract_id);
        client.set_execution_hub(&admin, &hub);
        assert_eq!(client.get_execution_hub(), hub);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Listings
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_create_listing() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);
        let seller = Address::generate(&env);
        seed_agent(&env, &contract_id, 1, &seller);

        let client = MarketplaceClient::new(&env, &contract_id);
        let listing_id = client.create_listing(&1u64, &seller, &0u32, &1_000_000i128, &None);
        assert_eq!(listing_id, 1u64);
        let listing = client.get_listing(&listing_id);
        assert!(listing.active);
        assert_eq!(listing.seller, seller);
    }

    #[test]
    #[should_panic(expected = "Agent already locked in escrow")]
    fn test_create_listing_already_locked() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);
        let seller = Address::generate(&env);
        let holder = Address::generate(&env);
        env.as_contract(&contract_id, || {
            let key = (String::from_str(&env, stellai_lib::AGENT_KEY_PREFIX), 2u64);
            env.storage().instance().set(
                &key,
                &stellai_lib::Agent {
                    id: 2,
                    owner: seller.clone(),
                    name: String::from_str(&env, "B"),
                    model_hash: String::from_str(&env, "h"),
                    metadata_cid: String::from_str(&env, "c"),
                    capabilities: Vec::new(&env),
                    evolution_level: 0,
                    created_at: 0,
                    updated_at: 0,
                    nonce: 0,
                    escrow_locked: true,
                    escrow_holder: Some(holder),
                },
            );
        });
        MarketplaceClient::new(&env, &contract_id)
            .create_listing(&2u64, &seller, &0u32, &500i128, &None);
    }

    #[test]
    #[should_panic(expected = "Price out of valid range")]
    fn test_negative_price_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);
        let seller = Address::generate(&env);
        seed_agent(&env, &contract_id, 3, &seller);
        MarketplaceClient::new(&env, &contract_id)
            .create_listing(&3u64, &seller, &0u32, &-1i128, &None);
    }

    #[test]
    fn test_cancel_listing() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);
        let seller = Address::generate(&env);
        seed_agent(&env, &contract_id, 4, &seller);
        let client = MarketplaceClient::new(&env, &contract_id);
        let lid = client.create_listing(&4u64, &seller, &0u32, &2_000i128, &None);
        assert!(client.get_listing(&lid).active);
        client.cancel_listing(&lid, &seller);
        assert!(!client.get_listing(&lid).active);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Royalties
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_set_and_get_royalty() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        seed_agent(&env, &contract_id, 5, &creator);
        let client = MarketplaceClient::new(&env, &contract_id);
        client.set_royalty(&5u64, &creator, &recipient, &500u32);
        let info = client.get_royalty(&5u64).unwrap();
        assert_eq!(info.fee, 500u32);
    }

    #[test]
    #[should_panic(expected = "Royalty exceeds maximum")]
    fn test_royalty_cap_enforced() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);
        let creator = Address::generate(&env);
        let recipient = Address::generate(&env);
        seed_agent(&env, &contract_id, 6, &creator);
        MarketplaceClient::new(&env, &contract_id)
            .set_royalty(&6u64, &creator, &recipient, &20_000u32);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Step functions (direct invocation)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_verify_sale_step() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);
        let seller = Address::generate(&env);
        let buyer = Address::generate(&env);

        env.as_contract(&contract_id, || {
            let mp = contract_id.clone();
            let ak = (String::from_str(&env, stellai_lib::AGENT_KEY_PREFIX), 10u64);
            env.storage().instance().set(
                &ak,
                &stellai_lib::Agent {
                    id: 10,
                    owner: seller.clone(),
                    name: String::from_str(&env, "V"),
                    model_hash: String::from_str(&env, "h"),
                    metadata_cid: String::from_str(&env, "c"),
                    capabilities: Vec::new(&env),
                    evolution_level: 0,
                    created_at: 0,
                    updated_at: 0,
                    nonce: 0,
                    escrow_locked: true,
                    escrow_holder: Some(mp),
                },
            );
            let lk = (String::from_str(&env, LISTING_PREFIX), 1u64);
            env.storage().instance().set(
                &lk,
                &stellai_lib::Listing {
                    listing_id: 1,
                    asset_id: 10,
                    asset_type: stellai_lib::AssetType::Agent,
                    seller: seller.clone(),
                    price: 100,
                    listing_type: stellai_lib::ListingType::Sale,
                    active: true,
                    created_at: 0,
                },
            );
            let psk = (String::from_str(&env, PENDING_SALE_PREFIX), 1u64);
            env.storage().instance().set(
                &psk,
                &PendingSale {
                    listing_id: 1,
                    buyer: buyer.clone(),
                    amount: 200,
                    seller: seller.clone(),
                    agent_id: 10,
                    workflow_id: 1,
                    created_at: 0,
                },
            );
        });

        let client = MarketplaceClient::new(&env, &contract_id);
        client.verify_sale(&Bytes::from_array(&env, &1u64.to_be_bytes()));
    }

    #[test]
    fn test_transfer_ownership_step() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);
        let seller = Address::generate(&env);
        let buyer = Address::generate(&env);

        env.as_contract(&contract_id, || {
            let mp = contract_id.clone();
            let ak = (String::from_str(&env, stellai_lib::AGENT_KEY_PREFIX), 11u64);
            env.storage().instance().set(
                &ak,
                &stellai_lib::Agent {
                    id: 11,
                    owner: seller.clone(),
                    name: String::from_str(&env, "T"),
                    model_hash: String::from_str(&env, "h"),
                    metadata_cid: String::from_str(&env, "c"),
                    capabilities: Vec::new(&env),
                    evolution_level: 0,
                    created_at: 0,
                    updated_at: 0,
                    nonce: 0,
                    escrow_locked: true,
                    escrow_holder: Some(mp),
                },
            );
            let lk = (String::from_str(&env, LISTING_PREFIX), 2u64);
            env.storage().instance().set(
                &lk,
                &stellai_lib::Listing {
                    listing_id: 2,
                    asset_id: 11,
                    asset_type: stellai_lib::AssetType::Agent,
                    seller: seller.clone(),
                    price: 100,
                    listing_type: stellai_lib::ListingType::Sale,
                    active: true,
                    created_at: 0,
                },
            );
            let psk = (String::from_str(&env, PENDING_SALE_PREFIX), 2u64);
            env.storage().instance().set(
                &psk,
                &PendingSale {
                    listing_id: 2,
                    buyer: buyer.clone(),
                    amount: 200,
                    seller: seller.clone(),
                    agent_id: 11,
                    workflow_id: 2,
                    created_at: 0,
                },
            );
        });

        MarketplaceClient::new(&env, &contract_id)
            .transfer_ownership(&Bytes::from_array(&env, &2u64.to_be_bytes()));

        env.as_contract(&contract_id, || {
            let ak = (String::from_str(&env, stellai_lib::AGENT_KEY_PREFIX), 11u64);
            let agent: stellai_lib::Agent = env.storage().instance().get(&ak).unwrap();
            assert_eq!(agent.owner, buyer);
        });
    }

    #[test]
    fn test_record_sale_step() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);
        let seller = Address::generate(&env);
        let buyer = Address::generate(&env);

        env.as_contract(&contract_id, || {
            let mp = contract_id.clone();
            let ak = (String::from_str(&env, stellai_lib::AGENT_KEY_PREFIX), 12u64);
            env.storage().instance().set(
                &ak,
                &stellai_lib::Agent {
                    id: 12,
                    owner: buyer.clone(),
                    name: String::from_str(&env, "R"),
                    model_hash: String::from_str(&env, "h"),
                    metadata_cid: String::from_str(&env, "c"),
                    capabilities: Vec::new(&env),
                    evolution_level: 0,
                    created_at: 0,
                    updated_at: 0,
                    nonce: 1,
                    escrow_locked: true,
                    escrow_holder: Some(mp),
                },
            );
            let lk = (String::from_str(&env, LISTING_PREFIX), 3u64);
            env.storage().instance().set(
                &lk,
                &stellai_lib::Listing {
                    listing_id: 3,
                    asset_id: 12,
                    asset_type: stellai_lib::AssetType::Agent,
                    seller: seller.clone(),
                    price: 100,
                    listing_type: stellai_lib::ListingType::Sale,
                    active: true,
                    created_at: 0,
                },
            );
            let psk = (String::from_str(&env, PENDING_SALE_PREFIX), 3u64);
            env.storage().instance().set(
                &psk,
                &PendingSale {
                    listing_id: 3,
                    buyer: buyer.clone(),
                    amount: 200,
                    seller: seller.clone(),
                    agent_id: 12,
                    workflow_id: 3,
                    created_at: 0,
                },
            );
        });

        MarketplaceClient::new(&env, &contract_id)
            .record_sale(&Bytes::from_array(&env, &3u64.to_be_bytes()));

        env.as_contract(&contract_id, || {
            let lk = (String::from_str(&env, LISTING_PREFIX), 3u64);
            let listing: stellai_lib::Listing = env.storage().instance().get(&lk).unwrap();
            assert!(!listing.active);

            let ak = (String::from_str(&env, stellai_lib::AGENT_KEY_PREFIX), 12u64);
            let agent: stellai_lib::Agent = env.storage().instance().get(&ak).unwrap();
            assert!(!agent.escrow_locked);
            assert!(agent.escrow_holder.is_none());

            let psk = (String::from_str(&env, PENDING_SALE_PREFIX), 3u64);
            assert!(!env.storage().instance().has(&psk));
        });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Rollback
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_rollback_restores_seller() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);
        let seller = Address::generate(&env);
        let buyer = Address::generate(&env);

        env.as_contract(&contract_id, || {
            let mp = contract_id.clone();
            let ak = (String::from_str(&env, stellai_lib::AGENT_KEY_PREFIX), 20u64);
            env.storage().instance().set(
                &ak,
                &stellai_lib::Agent {
                    id: 20,
                    owner: buyer.clone(), // ownership already xferred
                    name: String::from_str(&env, "Rb"),
                    model_hash: String::from_str(&env, "rb"),
                    metadata_cid: String::from_str(&env, "rbc"),
                    capabilities: Vec::new(&env),
                    evolution_level: 0,
                    created_at: 0,
                    updated_at: 0,
                    nonce: 1,
                    escrow_locked: true,
                    escrow_holder: Some(mp),
                },
            );
            let lk = (String::from_str(&env, LISTING_PREFIX), 10u64);
            env.storage().instance().set(
                &lk,
                &stellai_lib::Listing {
                    listing_id: 10,
                    asset_id: 20,
                    asset_type: stellai_lib::AssetType::Agent,
                    seller: seller.clone(),
                    price: 300,
                    listing_type: stellai_lib::ListingType::Sale,
                    active: true,
                    created_at: 0,
                },
            );
            let psk = (String::from_str(&env, PENDING_SALE_PREFIX), 10u64);
            env.storage().instance().set(
                &psk,
                &PendingSale {
                    listing_id: 10,
                    buyer: buyer.clone(),
                    amount: 300,
                    seller: seller.clone(),
                    agent_id: 20,
                    workflow_id: 99,
                    created_at: 0,
                },
            );
        });

        MarketplaceClient::new(&env, &contract_id)
            .rollback(&Bytes::from_array(&env, &10u64.to_be_bytes()));

        env.as_contract(&contract_id, || {
            let ak = (String::from_str(&env, stellai_lib::AGENT_KEY_PREFIX), 20u64);
            let agent: stellai_lib::Agent = env.storage().instance().get(&ak).unwrap();
            assert_eq!(agent.owner, seller);
            assert!(!agent.escrow_locked);
        });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Callback
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_callback_success_cleans_up() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);

        env.as_contract(&contract_id, || {
            let wlk = (String::from_str(&env, WF_LISTING_PREFIX), 7u64);
            env.storage().instance().set(&wlk, &5u64);
            let lk = (String::from_str(&env, LISTING_PREFIX), 5u64);
            env.storage().instance().set(
                &lk,
                &stellai_lib::Listing {
                    listing_id: 5,
                    asset_id: 99,
                    asset_type: stellai_lib::AssetType::Agent,
                    seller: Address::generate(&env),
                    price: 100,
                    listing_type: stellai_lib::ListingType::Sale,
                    active: false,
                    created_at: 0,
                },
            );
        });

        MarketplaceClient::new(&env, &contract_id).wf_done(&7u64, &2u32);

        env.as_contract(&contract_id, || {
            let wlk = (String::from_str(&env, WF_LISTING_PREFIX), 7u64);
            assert!(!env.storage().instance().has(&wlk));
        });
    }

    #[test]
    fn test_callback_failure_reactivates_listing() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _) = setup_marketplace(&env);

        env.as_contract(&contract_id, || {
            let wlk = (String::from_str(&env, WF_LISTING_PREFIX), 8u64);
            env.storage().instance().set(&wlk, &6u64);
            let lk = (String::from_str(&env, LISTING_PREFIX), 6u64);
            env.storage().instance().set(
                &lk,
                &stellai_lib::Listing {
                    listing_id: 6,
                    asset_id: 50,
                    asset_type: stellai_lib::AssetType::Agent,
                    seller: Address::generate(&env),
                    price: 100,
                    listing_type: stellai_lib::ListingType::Sale,
                    active: false,
                    created_at: 0,
                },
            );
        });

        MarketplaceClient::new(&env, &contract_id).wf_done(&8u64, &4u32);

        env.as_contract(&contract_id, || {
            let lk = (String::from_str(&env, LISTING_PREFIX), 6u64);
            let listing: stellai_lib::Listing = env.storage().instance().get(&lk).unwrap();
            assert!(listing.active);
        });
    }
}