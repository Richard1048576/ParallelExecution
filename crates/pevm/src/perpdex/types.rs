//! PerpDex transaction types and operations.

use alloy_primitives::{Address, U256};

/// A PerpDex transaction containing one or more operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerpDexTx {
    /// The user executing this transaction
    pub user: Address,
    /// List of operations to execute atomically
    pub operations: Vec<Operation>,
    /// Gas limit for this transaction
    pub gas_limit: u64,
    /// Gas price in wei
    pub gas_price: U256,
    /// Transaction nonce
    pub nonce: u64,
    /// Optional signature (for verification)
    pub signature: Option<[u8; 65]>,
}

/// A single operation within a PerpDex transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Place a new order in the orderbook
    PlaceOrder {
        /// Market identifier
        market_id: u64,
        /// Type of order
        order_type: OrderType,
        /// Long or Short
        side: Side,
        /// Order price (18 decimal fixed point)
        price: U256,
        /// Order size (18 decimal fixed point)
        size: U256,
        /// Leverage multiplier (1-100x)
        leverage: u8,
        /// Only place order if it goes on the orderbook (no immediate execution)
        post_only: bool,
        /// Only reduce existing position (cannot increase position size)
        reduce_only: bool,
        /// Optional expiration timestamp
        expires_at: Option<u64>,
    },
    /// Cancel specific orders
    CancelOrder {
        /// Market identifier
        market_id: u64,
        /// Order IDs to cancel
        order_ids: Vec<u64>,
    },
    /// Cancel all orders (optionally filtered by market)
    CancelAll {
        /// Optional market filter (None = all markets)
        market_id: Option<u64>,
    },
    /// Set leverage for a market
    SetLeverage {
        /// Market identifier
        market_id: u64,
        /// New leverage (1-100x)
        leverage: u8,
    },
    /// Adjust margin for a position
    AdjustMargin {
        /// Market identifier
        market_id: u64,
        /// Amount to add (positive) or remove (negative)
        amount: U256,
    },
}

/// Type of order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderType {
    /// Limit order: execute at specified price or better
    Limit,
    /// Market order: execute immediately at best available price
    Market,
    /// Stop-limit order: become limit order when stop price is reached
    StopLimit,
    /// Stop-market order: become market order when stop price is reached
    StopMarket,
}

/// Order side
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// Long position (buy)
    Long,
    /// Short position (sell)
    Short,
}

/// Order status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderStatus {
    /// Order is active in the orderbook
    Open,
    /// Order partially filled
    PartiallyFilled,
    /// Order completely filled
    Filled,
    /// Order cancelled by user
    Cancelled,
    /// Order expired
    Expired,
}

/// Result of a single operation execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationResult {
    /// Order was placed
    OrderPlaced {
        /// Generated order ID
        order_id: u64,
        /// Amount filled immediately
        filled_size: U256,
    },
    /// Orders were cancelled
    OrdersCancelled {
        /// IDs of cancelled orders
        order_ids: Vec<u64>,
    },
    /// Leverage was updated
    LeverageUpdated {
        /// Market ID
        market_id: u64,
        /// New leverage
        leverage: u8,
    },
    /// Margin was adjusted
    MarginAdjusted {
        /// Market ID
        market_id: u64,
        /// Amount adjusted
        amount: U256,
    },
}

/// Result of executing a complete PerpDex transaction
#[derive(Debug, Clone)]
pub struct PerpDexResult {
    /// Gas used
    pub gas_used: u64,
    /// Results of each operation
    pub operations_executed: Vec<OperationResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_sizes() {
        // Ensure enums don't grow too large
        assert!(std::mem::size_of::<Operation>() <= 256);
        assert!(std::mem::size_of::<OperationResult>() <= 128);
    }

    #[test]
    fn test_order_type_basics() {
        assert_eq!(OrderType::Limit, OrderType::Limit);
        assert_ne!(OrderType::Limit, OrderType::Market);
    }

    #[test]
    fn test_side_basics() {
        assert_eq!(Side::Long, Side::Long);
        assert_ne!(Side::Long, Side::Short);
    }
}
