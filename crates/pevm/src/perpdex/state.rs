//! PerpDex state locations and values.

use std::hash::{Hash, Hasher};

use alloy_primitives::{Address, U256};
use rustc_hash::FxHasher;

use super::state_traits::{PartitionId, StateLocation, StateValue};
use super::types::{OrderStatus, OrderType, Side};

/// State locations in the PerpDex VM.
///
/// Each location represents a unique piece of state in the system.
/// The partition ID ensures zero collision with EVM state.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PerpDexLocation {
    /// User balance for a specific asset
    /// Format: (user_address, asset_id)
    Balance(Address, u16),

    /// User's position in a market
    /// Format: (user_address, market_id)
    Position(Address, u64),

    /// A specific order
    /// Format: (user_address, market_id, order_id)
    Order(Address, u64, u64),

    /// Global market state
    /// Format: (market_id)
    MarketState(u64),

    /// User's transaction nonce
    /// Format: (user_address)
    UserNonce(Address),
}

impl StateLocation for PerpDexLocation {
    fn location_hash(&self) -> u64 {
        let mut hasher = FxHasher::default();
        self.hash(&mut hasher);
        let base_hash = hasher.finish();

        // Mix with partition ID (high 8 bits = 0x02)
        // This ensures zero collision with EVM state (high 8 bits = 0x00)
        self.partition_id().mix_hash(base_hash)
    }

    fn partition_id(&self) -> PartitionId {
        PartitionId::PERPDEX
    }
}

/// State values in the PerpDex VM.
#[derive(Clone, Debug)]
pub enum PerpDexValue {
    /// User's balance for an asset
    Balance(U256),

    /// Position data
    Position {
        /// Position size (positive = long, negative = short)
        size: i128,
        /// Entry price (18 decimal fixed point)
        entry_price: U256,
        /// Leverage multiplier (1-100x)
        leverage: u8,
        /// Margin locked for this position
        margin: U256,
        /// Unrealized PnL
        unrealized_pnl: i128,
        /// Last funding rate index applied
        last_funding_index: U256,
    },

    /// Order data
    Order {
        /// Type of order
        order_type: OrderType,
        /// Side (Long/Short)
        side: Side,
        /// Limit price
        price: U256,
        /// Total size
        size: U256,
        /// Amount filled so far
        filled: U256,
        /// Current status
        status: OrderStatus,
        /// Post-only flag
        post_only: bool,
        /// Reduce-only flag
        reduce_only: bool,
        /// Creation timestamp
        created_at: u64,
        /// Optional expiration
        expires_at: Option<u64>,
    },

    /// Market state
    MarketState {
        /// Index price from oracle
        index_price: U256,
        /// Mark price used for PnL calculation
        mark_price: U256,
        /// Current funding rate
        funding_rate: i128,
        /// Cumulative funding index
        funding_index: U256,
        /// Total open interest (long positions)
        open_interest_long: U256,
        /// Total open interest (short positions)
        open_interest_short: U256,
        /// Last update timestamp
        last_update: u64,
    },

    /// User nonce
    Nonce(u64),

    /// Lazy funding rate update
    ///
    /// This is applied at block finalization to avoid creating artificial
    /// dependencies between all positions in the same market.
    LazyFunding {
        /// Market identifier
        market_id: u64,
        /// Position size at time of funding
        position_size: i128,
    },

    /// Deleted/cleared state
    Deleted,
}

impl StateValue for PerpDexValue {
    fn is_lazy(&self) -> bool {
        matches!(self, PerpDexValue::LazyFunding { .. })
    }

    fn is_deleted(&self) -> bool {
        matches!(self, PerpDexValue::Deleted)
    }
}

/// Helper to create a default position
impl PerpDexValue {
    /// Create an empty position
    pub fn empty_position() -> Self {
        PerpDexValue::Position {
            size: 0,
            entry_price: U256::ZERO,
            leverage: 1,
            margin: U256::ZERO,
            unrealized_pnl: 0,
            last_funding_index: U256::ZERO,
        }
    }

    /// Create a default market state
    pub fn default_market_state(timestamp: u64) -> Self {
        PerpDexValue::MarketState {
            index_price: U256::ZERO,
            mark_price: U256::ZERO,
            funding_rate: 0,
            funding_index: U256::ZERO,
            open_interest_long: U256::ZERO,
            open_interest_short: U256::ZERO,
            last_update: timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_partition_id_isolation() {
        let balance_loc = PerpDexLocation::Balance(address!("0000000000000000000000000000000000000001"), 0);
        let position_loc = PerpDexLocation::Position(address!("0000000000000000000000000000000000000001"), 1);

        assert_eq!(balance_loc.partition_id(), PartitionId::PERPDEX);
        assert_eq!(position_loc.partition_id(), PartitionId::PERPDEX);

        // Hashes should have high 8 bits = 2 (PERPDEX partition)
        let balance_hash = balance_loc.location_hash();
        let position_hash = position_loc.location_hash();

        assert_eq!(balance_hash >> 56, 2);
        assert_eq!(position_hash >> 56, 2);
        assert_ne!(balance_hash, position_hash);
    }

    #[test]
    fn test_zero_collision_with_evm() {
        use crate::perpdex::state_traits::PartitionId;

        let perpdex_loc = PerpDexLocation::Balance(address!("0000000000000000000000000000000000000001"), 0);
        let perpdex_hash = perpdex_loc.location_hash();

        // Simulate an EVM hash with same base value
        let base = perpdex_hash & 0x00FF_FFFF_FFFF_FFFF;
        let evm_hash = PartitionId::EVM.mix_hash(base);

        assert_ne!(perpdex_hash, evm_hash);
        assert_eq!(perpdex_hash >> 56, 2);
        assert_eq!(evm_hash >> 56, 0);
    }

    #[test]
    fn test_state_value_lazy() {
        let balance = PerpDexValue::Balance(U256::from(1000));
        let lazy_funding = PerpDexValue::LazyFunding {
            market_id: 1,
            position_size: 100,
        };

        assert!(!balance.is_lazy());
        assert!(lazy_funding.is_lazy());
    }

    #[test]
    fn test_state_value_deleted() {
        let balance = PerpDexValue::Balance(U256::from(1000));
        let deleted = PerpDexValue::Deleted;

        assert!(!balance.is_deleted());
        assert!(deleted.is_deleted());
    }

    #[test]
    fn test_empty_position() {
        if let PerpDexValue::Position { size, entry_price, leverage, margin, unrealized_pnl, last_funding_index } = PerpDexValue::empty_position() {
            assert_eq!(size, 0);
            assert_eq!(entry_price, U256::ZERO);
            assert_eq!(leverage, 1);
            assert_eq!(margin, U256::ZERO);
            assert_eq!(unrealized_pnl, 0);
            assert_eq!(last_funding_index, U256::ZERO);
        } else {
            panic!("empty_position should return Position variant");
        }
    }
}
