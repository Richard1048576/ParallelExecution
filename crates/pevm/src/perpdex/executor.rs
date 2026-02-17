//! PerpDex executor implementation.

use std::sync::Arc;

use alloy_primitives::{Address, U256};

use super::executor_traits::{
    FinishExecFlags, MvMemory, ReadSet, TxVersion, VmExecutionResult, VmExecutor,
    WriteSet,
};
use super::state::{PerpDexLocation, PerpDexValue};
use super::storage::{PerpDexStorage, StorageError};
use super::types::{
    Operation, OperationResult, OrderStatus, OrderType, PerpDexResult, PerpDexTx, Side,
};

/// Configuration for block execution context.
#[derive(Debug, Clone)]
pub struct BlockConfig {
    /// Block timestamp
    pub timestamp: u64,
    /// Block number
    pub block_number: u64,
}

/// Execution error types.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ExecutionError {
    /// Nonce mismatch
    #[error("Nonce mismatch: expected {expected}, got {actual}")]
    NonceMismatch { expected: u64, actual: u64 },

    /// Insufficient balance
    #[error("Insufficient balance: required {required}, available {available}")]
    InsufficientBalance { required: U256, available: U256 },

    /// Invalid leverage
    #[error("Invalid leverage: {0} (must be 1-100)")]
    InvalidLeverage(u8),

    /// Reduce-only violation
    #[error("Reduce-only order would increase position size")]
    ReduceOnlyViolation,

    /// Invalid position
    #[error("Invalid position operation: {0}")]
    InvalidPosition(String),

    /// Storage error
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    /// Order not found
    #[error("Order not found: {0}")]
    OrderNotFound(u64),
}

/// PerpDex executor for parallel transaction execution.
pub struct PerpDexExecutor<S> {
    storage: Arc<S>,
    block_config: BlockConfig,
    next_order_id: std::sync::atomic::AtomicU64,
}

impl<S> std::fmt::Debug for PerpDexExecutor<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PerpDexExecutor")
            .field("block_config", &self.block_config)
            .field("next_order_id", &self.next_order_id.load(std::sync::atomic::Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl<S> PerpDexExecutor<S> {
    /// Create a new PerpDex executor.
    pub fn new(storage: Arc<S>, block_config: BlockConfig) -> Self {
        Self {
            storage,
            block_config,
            next_order_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Generate a unique order ID.
    fn generate_order_id(&self) -> u64 {
        self.next_order_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }
}

impl<S: PerpDexStorage> PerpDexExecutor<S> {
    /// Read a value from MV memory or storage (placeholder for Phase 3).
    fn read_location(
        &self,
        location: &PerpDexLocation,
        _read_set: &mut ReadSet<PerpDexLocation>,
        _mv_memory: &MvMemory,
    ) -> Result<Option<PerpDexValue>, ExecutionError> {
        // Phase 2: Simple storage read
        // Phase 3: Will integrate with MvMemory for parallel execution
        self.storage.get(location).map_err(|e| e.into())
    }

    /// Read user nonce.
    fn read_nonce(
        &self,
        user: Address,
        read_set: &mut ReadSet<PerpDexLocation>,
        mv_memory: &MvMemory,
    ) -> Result<u64, ExecutionError> {
        let location = PerpDexLocation::UserNonce(user);
        match self.read_location(&location, read_set, mv_memory)? {
            Some(PerpDexValue::Nonce(n)) => Ok(n),
            None => Ok(0), // Default nonce
            _ => Err(ExecutionError::InvalidPosition(
                "Invalid nonce value".to_string(),
            )),
        }
    }

    /// Read user balance.
    fn read_balance(
        &self,
        user: Address,
        asset_id: u16,
        read_set: &mut ReadSet<PerpDexLocation>,
        mv_memory: &MvMemory,
    ) -> Result<U256, ExecutionError> {
        let location = PerpDexLocation::Balance(user, asset_id);
        match self.read_location(&location, read_set, mv_memory)? {
            Some(PerpDexValue::Balance(b)) => Ok(b),
            None => Ok(U256::ZERO),
            _ => Err(ExecutionError::InvalidPosition(
                "Invalid balance value".to_string(),
            )),
        }
    }

    /// Read user position.
    fn read_position(
        &self,
        user: Address,
        market_id: u64,
        read_set: &mut ReadSet<PerpDexLocation>,
        mv_memory: &MvMemory,
    ) -> Result<Option<PositionData>, ExecutionError> {
        let location = PerpDexLocation::Position(user, market_id);
        match self.read_location(&location, read_set, mv_memory)? {
            Some(PerpDexValue::Position {
                size,
                entry_price,
                leverage,
                margin,
                unrealized_pnl,
                last_funding_index,
            }) => Ok(Some(PositionData {
                size,
                entry_price,
                leverage,
                margin,
                unrealized_pnl,
                last_funding_index,
            })),
            None => Ok(None),
            _ => Err(ExecutionError::InvalidPosition(
                "Invalid position value".to_string(),
            )),
        }
    }

    /// Read market state.
    fn read_market(
        &self,
        market_id: u64,
        read_set: &mut ReadSet<PerpDexLocation>,
        mv_memory: &MvMemory,
    ) -> Result<MarketData, ExecutionError> {
        let location = PerpDexLocation::MarketState(market_id);
        match self.read_location(&location, read_set, mv_memory)? {
            Some(PerpDexValue::MarketState {
                index_price,
                mark_price,
                funding_rate,
                funding_index,
                open_interest_long,
                open_interest_short,
                last_update,
            }) => Ok(MarketData {
                index_price,
                mark_price,
                funding_rate,
                funding_index,
                open_interest_long,
                open_interest_short,
                last_update,
            }),
            None => {
                // Default market state
                Ok(MarketData {
                    index_price: U256::from(1000) * U256::from(10u128.pow(18)), // $1000
                    mark_price: U256::from(1000) * U256::from(10u128.pow(18)),
                    funding_rate: 0,
                    funding_index: U256::ZERO,
                    open_interest_long: U256::ZERO,
                    open_interest_short: U256::ZERO,
                    last_update: self.block_config.timestamp,
                })
            }
            _ => Err(ExecutionError::InvalidPosition(
                "Invalid market state".to_string(),
            )),
        }
    }
}

/// Helper structs for cleaner code
#[derive(Debug, Clone)]
struct PositionData {
    size: i128,
    entry_price: U256,
    leverage: u8,
    margin: U256,
    unrealized_pnl: i128,
    last_funding_index: U256,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MarketData {
    index_price: U256,
    mark_price: U256,
    funding_rate: i128,
    funding_index: U256,
    open_interest_long: U256,
    open_interest_short: U256,
    last_update: u64,
}

// Constants
const USDC_ASSET_ID: u16 = 0;
const PRICE_PRECISION: u128 = 10u128.pow(18); // 18 decimals

impl<S: PerpDexStorage> VmExecutor for PerpDexExecutor<S> {
    type Location = PerpDexLocation;
    type Value = PerpDexValue;
    type Transaction = PerpDexTx;
    type Result = PerpDexResult;
    type Storage = S;

    fn execute(
        &self,
        _tx_version: &TxVersion,
        tx: &PerpDexTx,
        _storage: &S,
        mv_memory: &MvMemory,
    ) -> VmExecutionResult<PerpDexLocation, PerpDexValue, PerpDexResult> {
        let mut read_set = ReadSet::new();
        let mut write_set = WriteSet::new();
        let mut results = Vec::new();
        let mut gas_used = 0u64;

        // 1. Verify nonce
        let current_nonce = match self.read_nonce(tx.user, &mut read_set, mv_memory) {
            Ok(n) => n,
            Err(e) => return VmExecutionResult::Error(Box::new(e)),
        };

        if current_nonce != tx.nonce {
            return VmExecutionResult::Error(Box::new(ExecutionError::NonceMismatch {
                expected: current_nonce,
                actual: tx.nonce,
            }));
        }

        // 2. Execute each operation
        for op in &tx.operations {
            let result = match op {
                Operation::PlaceOrder {
                    market_id,
                    order_type,
                    side,
                    price,
                    size,
                    leverage,
                    post_only,
                    reduce_only,
                    expires_at,
                } => self.execute_place_order(
                    tx,
                    *market_id,
                    *order_type,
                    *side,
                    *price,
                    *size,
                    *leverage,
                    *post_only,
                    *reduce_only,
                    *expires_at,
                    &mut read_set,
                    &mut write_set,
                    mv_memory,
                ),
                Operation::CancelOrder {
                    market_id,
                    order_ids,
                } => self.execute_cancel_order(tx, *market_id, order_ids, &mut write_set),
                Operation::CancelAll { market_id } => {
                    self.execute_cancel_all(tx, *market_id, &mut read_set, &mut write_set, mv_memory)
                }
                Operation::SetLeverage {
                    market_id,
                    leverage,
                } => self.execute_set_leverage(tx, *market_id, *leverage, &mut write_set),
                Operation::AdjustMargin { market_id, amount } => {
                    self.execute_adjust_margin(tx, *market_id, *amount, &mut read_set, &mut write_set, mv_memory)
                }
            };

            match result {
                Ok(r) => {
                    gas_used += self.estimate_gas(op);
                    results.push(r);
                }
                Err(e) => return VmExecutionResult::Error(Box::new(e)),
            }
        }

        // 3. Update nonce
        write_set.push(
            PerpDexLocation::UserNonce(tx.user),
            PerpDexValue::Nonce(current_nonce + 1),
        );

        // 4. Determine flags
        let flags = if read_set.is_empty() {
            FinishExecFlags::empty()
        } else {
            FinishExecFlags::NeedValidation
        };

        VmExecutionResult::Success {
            read_set,
            write_set,
            result: PerpDexResult {
                gas_used,
                operations_executed: results,
            },
            flags,
        }
    }

    fn get_lazy_locations(&self, tx: &PerpDexTx) -> Vec<PerpDexLocation> {
        // Lazy locations: positions that need funding rate updates
        tx.operations
            .iter()
            .filter_map(|op| match op {
                Operation::PlaceOrder { market_id, .. } => {
                    Some(PerpDexLocation::Position(tx.user, *market_id))
                }
                _ => None,
            })
            .collect()
    }
}

// Continue with operation implementations in next part...
impl<S: PerpDexStorage> PerpDexExecutor<S> {
    /// Estimate gas for an operation (simplified).
    fn estimate_gas(&self, _op: &Operation) -> u64 {
        // Simplified gas estimation
        // TODO: More accurate gas model
        21000
    }

    /// Execute SetLeverage operation.
    fn execute_set_leverage(
        &self,
        _tx: &PerpDexTx,
        market_id: u64,
        leverage: u8,
        _write_set: &mut WriteSet<PerpDexLocation, PerpDexValue>,
    ) -> Result<OperationResult, ExecutionError> {
        if leverage < 1 || leverage > 100 {
            return Err(ExecutionError::InvalidLeverage(leverage));
        }

        // Note: In a full implementation, we'd read the current position
        // and update its leverage. For Phase 2, we just record the intent.

        Ok(OperationResult::LeverageUpdated {
            market_id,
            leverage,
        })
    }

    /// Execute AdjustMargin operation.
    fn execute_adjust_margin(
        &self,
        tx: &PerpDexTx,
        market_id: u64,
        amount: U256,
        read_set: &mut ReadSet<PerpDexLocation>,
        write_set: &mut WriteSet<PerpDexLocation, PerpDexValue>,
        mv_memory: &MvMemory,
    ) -> Result<OperationResult, ExecutionError> {
        // Read current balance
        let balance = self.read_balance(tx.user, USDC_ASSET_ID, read_set, mv_memory)?;

        // Check sufficient balance for adding margin
        if amount > balance {
            return Err(ExecutionError::InsufficientBalance {
                required: amount,
                available: balance,
            });
        }

        // Read current position
        let position = self.read_position(tx.user, market_id, read_set, mv_memory)?;

        if let Some(mut pos) = position {
            // Add margin to position
            pos.margin += amount;

            // Write updated position
            write_set.push(
                PerpDexLocation::Position(tx.user, market_id),
                PerpDexValue::Position {
                    size: pos.size,
                    entry_price: pos.entry_price,
                    leverage: pos.leverage,
                    margin: pos.margin,
                    unrealized_pnl: pos.unrealized_pnl,
                    last_funding_index: pos.last_funding_index,
                },
            );

            // Deduct from balance
            write_set.push(
                PerpDexLocation::Balance(tx.user, USDC_ASSET_ID),
                PerpDexValue::Balance(balance - amount),
            );

            Ok(OperationResult::MarginAdjusted { market_id, amount })
        } else {
            Err(ExecutionError::InvalidPosition(
                "No position to adjust margin".to_string(),
            ))
        }
    }

    /// Execute CancelOrder operation.
    fn execute_cancel_order(
        &self,
        tx: &PerpDexTx,
        market_id: u64,
        order_ids: &[u64],
        write_set: &mut WriteSet<PerpDexLocation, PerpDexValue>,
    ) -> Result<OperationResult, ExecutionError> {
        for &order_id in order_ids {
            let order_loc = PerpDexLocation::Order(tx.user, market_id, order_id);
            write_set.push(order_loc, PerpDexValue::Deleted);
        }

        Ok(OperationResult::OrdersCancelled {
            order_ids: order_ids.to_vec(),
        })
    }

    /// Execute CancelAll operation.
    fn execute_cancel_all(
        &self,
        tx: &PerpDexTx,
        market_id: Option<u64>,
        _read_set: &mut ReadSet<PerpDexLocation>,
        write_set: &mut WriteSet<PerpDexLocation, PerpDexValue>,
        _mv_memory: &MvMemory,
    ) -> Result<OperationResult, ExecutionError> {
        // Simplified: In a full implementation, we'd query storage for all user orders
        // For Phase 2, we just demonstrate the pattern

        let order_ids = if let Some(mid) = market_id {
            self.storage.get_user_orders(tx.user, mid)?
        } else {
            self.storage.get_all_user_orders(tx.user)?
        };

        for order_id in &order_ids {
            let mid = market_id.unwrap_or(0); // Simplified
            let order_loc = PerpDexLocation::Order(tx.user, mid, *order_id);
            write_set.push(order_loc, PerpDexValue::Deleted);
        }

        Ok(OperationResult::OrdersCancelled { order_ids })
    }

    /// Execute PlaceOrder operation - the core order matching engine.
    #[allow(clippy::too_many_arguments)]
    fn execute_place_order(
        &self,
        tx: &PerpDexTx,
        market_id: u64,
        order_type: OrderType,
        side: Side,
        price: U256,
        size: U256,
        leverage: u8,
        post_only: bool,
        reduce_only: bool,
        expires_at: Option<u64>,
        read_set: &mut ReadSet<PerpDexLocation>,
        write_set: &mut WriteSet<PerpDexLocation, PerpDexValue>,
        mv_memory: &MvMemory,
    ) -> Result<OperationResult, ExecutionError> {
        // Validate leverage
        if leverage < 1 || leverage > 100 {
            return Err(ExecutionError::InvalidLeverage(leverage));
        }

        // 1. Read user balance
        let balance = self.read_balance(tx.user, USDC_ASSET_ID, read_set, mv_memory)?;

        // 2. Read current position
        let position = self.read_position(tx.user, market_id, read_set, mv_memory)?;

        // 3. Read market state
        let market = self.read_market(market_id, read_set, mv_memory)?;

        // 4. Calculate required margin
        let required_margin = self.calculate_required_margin(price, size, leverage)?;

        // Check balance
        if balance < required_margin {
            return Err(ExecutionError::InsufficientBalance {
                required: required_margin,
                available: balance,
            });
        }

        // 5. Match order based on type
        let (filled_size, avg_fill_price) = match order_type {
            OrderType::Market => {
                self.execute_market_order(side, size, &market, &position, reduce_only)?
            }
            OrderType::Limit => {
                if post_only {
                    // Post-only: no immediate execution
                    (U256::ZERO, U256::ZERO)
                } else {
                    self.execute_limit_order(side, price, size, &market, &position, reduce_only)?
                }
            }
            OrderType::StopLimit | OrderType::StopMarket => {
                // Stop orders: not triggered yet, just place on book
                (U256::ZERO, U256::ZERO)
            }
        };

        // 6. Generate order ID
        let order_id = self.generate_order_id();

        // 7. Place remaining size on orderbook
        let remaining = size - filled_size;
        if remaining > U256::ZERO {
            let order_loc = PerpDexLocation::Order(tx.user, market_id, order_id);
            write_set.push(
                order_loc,
                PerpDexValue::Order {
                    order_type,
                    side,
                    price,
                    size: remaining,
                    filled: filled_size,
                    status: if filled_size > U256::ZERO {
                        OrderStatus::PartiallyFilled
                    } else {
                        OrderStatus::Open
                    },
                    post_only,
                    reduce_only,
                    created_at: self.block_config.timestamp,
                    expires_at,
                },
            );
        }

        // 8. Update position if filled
        if filled_size > U256::ZERO {
            let new_position =
                self.update_position(position, side, filled_size, avg_fill_price, leverage)?;

            write_set.push(
                PerpDexLocation::Position(tx.user, market_id),
                PerpDexValue::Position {
                    size: new_position.size,
                    entry_price: new_position.entry_price,
                    leverage: new_position.leverage,
                    margin: new_position.margin,
                    unrealized_pnl: new_position.unrealized_pnl,
                    last_funding_index: new_position.last_funding_index,
                },
            );

            // Deduct margin from balance
            write_set.push(
                PerpDexLocation::Balance(tx.user, USDC_ASSET_ID),
                PerpDexValue::Balance(balance - required_margin),
            );
        }

        Ok(OperationResult::OrderPlaced {
            order_id,
            filled_size,
        })
    }

    /// Execute a market order - fills immediately at mark price.
    fn execute_market_order(
        &self,
        side: Side,
        size: U256,
        market: &MarketData,
        current_position: &Option<PositionData>,
        reduce_only: bool,
    ) -> Result<(U256, U256), ExecutionError> {
        let direction = match side {
            Side::Long => 1i128,
            Side::Short => -1i128,
        };
        let size_signed = size.try_into().unwrap_or(i128::MAX) * direction;

        // Check reduce_only constraint
        if reduce_only {
            if let Some(pos) = current_position {
                if !self.is_reducing(size_signed, pos.size) {
                    return Err(ExecutionError::ReduceOnlyViolation);
                }
            } else {
                // No position to reduce
                return Err(ExecutionError::ReduceOnlyViolation);
            }
        }

        // Market orders fill at mark price (simplified model)
        // In production: would match against orderbook
        Ok((size, market.mark_price))
    }

    /// Execute a limit order - fills at limit price or better.
    fn execute_limit_order(
        &self,
        side: Side,
        price: U256,
        size: U256,
        market: &MarketData,
        current_position: &Option<PositionData>,
        reduce_only: bool,
    ) -> Result<(U256, U256), ExecutionError> {
        // Simplified matching logic:
        // - Long limit: fills if mark_price <= limit_price
        // - Short limit: fills if mark_price >= limit_price

        let can_fill = match side {
            Side::Long => market.mark_price <= price,
            Side::Short => market.mark_price >= price,
        };

        if !can_fill {
            // Price not favorable, no fill
            return Ok((U256::ZERO, U256::ZERO));
        }

        // Check reduce_only
        if reduce_only {
            let direction = match side {
                Side::Long => 1i128,
                Side::Short => -1i128,
            };
            let size_signed = size.try_into().unwrap_or(i128::MAX) * direction;

            if let Some(pos) = current_position {
                if !self.is_reducing(size_signed, pos.size) {
                    return Err(ExecutionError::ReduceOnlyViolation);
                }
            } else {
                return Err(ExecutionError::ReduceOnlyViolation);
            }
        }

        // Fill at the better price (limit price)
        Ok((size, price))
    }

    /// Check if an order reduces position size.
    fn is_reducing(&self, order_size_signed: i128, position_size: i128) -> bool {
        // Reducing means order and position have opposite signs
        // or order size is less than position size
        if position_size == 0 {
            return false;
        }

        let same_sign = (order_size_signed > 0) == (position_size > 0);
        if same_sign {
            // Same direction = increasing
            false
        } else {
            // Opposite direction = reducing
            true
        }
    }

    /// Calculate required margin for a position.
    fn calculate_required_margin(
        &self,
        price: U256,
        size: U256,
        leverage: u8,
    ) -> Result<U256, ExecutionError> {
        // Margin = (price * size) / leverage
        // All values use 18 decimal precision

        let notional = price * size / U256::from(PRICE_PRECISION);
        let margin = notional / U256::from(leverage);

        Ok(margin)
    }

    /// Update position after a fill.
    fn update_position(
        &self,
        current: Option<PositionData>,
        side: Side,
        filled_size: U256,
        fill_price: U256,
        leverage: u8,
    ) -> Result<PositionData, ExecutionError> {
        let direction = match side {
            Side::Long => 1i128,
            Side::Short => -1i128,
        };
        let size_delta = filled_size.try_into().unwrap_or(i128::MAX) * direction;

        if let Some(pos) = current {
            // Update existing position
            let new_size = pos.size + size_delta;

            // Calculate new average entry price
            let new_entry_price = if pos.size.signum() == size_delta.signum() {
                // Adding to position
                let old_notional = U256::from(pos.size.unsigned_abs()) * pos.entry_price
                    / U256::from(PRICE_PRECISION);
                let new_notional = U256::from(size_delta.unsigned_abs()) * fill_price
                    / U256::from(PRICE_PRECISION);
                let total_notional = old_notional + new_notional;
                let total_size = U256::from(new_size.unsigned_abs());

                if total_size > U256::ZERO {
                    total_notional * U256::from(PRICE_PRECISION) / total_size
                } else {
                    U256::ZERO
                }
            } else {
                // Reducing or flipping position
                if new_size.abs() < pos.size.abs() {
                    // Partial close: keep old entry price
                    pos.entry_price
                } else {
                    // Flip: new entry price
                    fill_price
                }
            };

            // Calculate margin for new size
            let new_margin = self.calculate_required_margin(
                new_entry_price,
                U256::from(new_size.unsigned_abs()),
                leverage,
            )?;

            Ok(PositionData {
                size: new_size,
                entry_price: new_entry_price,
                leverage,
                margin: new_margin,
                unrealized_pnl: 0, // Reset PnL calculation
                last_funding_index: pos.last_funding_index,
            })
        } else {
            // New position
            let margin = self.calculate_required_margin(fill_price, filled_size, leverage)?;

            Ok(PositionData {
                size: size_delta,
                entry_price: fill_price,
                leverage,
                margin,
                unrealized_pnl: 0,
                last_funding_index: U256::ZERO,
            })
        }
    }

    /// Calculate unrealized PnL for a position.
    #[allow(dead_code)]
    fn calculate_unrealized_pnl(
        &self,
        position: &PositionData,
        mark_price: U256,
    ) -> i128 {
        if position.size == 0 {
            return 0;
        }

        let size_abs = U256::from(position.size.unsigned_abs());
        let is_long = position.size > 0;

        // PnL = size * (mark_price - entry_price) for long
        // PnL = size * (entry_price - mark_price) for short

        let price_diff = if mark_price > position.entry_price {
            let diff = mark_price - position.entry_price;
            if is_long {
                diff
            } else {
                return -((size_abs * diff / U256::from(PRICE_PRECISION))
                    .try_into()
                    .unwrap_or(i128::MAX));
            }
        } else {
            let diff = position.entry_price - mark_price;
            if is_long {
                return -((size_abs * diff / U256::from(PRICE_PRECISION))
                    .try_into()
                    .unwrap_or(i128::MAX));
            } else {
                diff
            }
        };

        (size_abs * price_diff / U256::from(PRICE_PRECISION))
            .try_into()
            .unwrap_or(i128::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_executor_creation() {
        use crate::perpdex::InMemoryStorage;

        let storage = Arc::new(InMemoryStorage::new());
        let config = BlockConfig {
            timestamp: 1000,
            block_number: 1,
        };
        let executor = PerpDexExecutor::new(storage, config);

        let order_id1 = executor.generate_order_id();
        let order_id2 = executor.generate_order_id();

        assert_eq!(order_id1, 1);
        assert_eq!(order_id2, 2);
    }

    #[test]
    fn test_calculate_required_margin() {
        use crate::perpdex::InMemoryStorage;

        let storage = Arc::new(InMemoryStorage::new());
        let config = BlockConfig {
            timestamp: 1000,
            block_number: 1,
        };
        let executor = PerpDexExecutor::new(storage, config);

        // Price: $1000, Size: 1.0, Leverage: 10x
        // Expected margin: $100

        let price = U256::from(1000) * U256::from(PRICE_PRECISION);
        let size = U256::from(1) * U256::from(PRICE_PRECISION);
        let leverage = 10;

        let margin = executor
            .calculate_required_margin(price, size, leverage)
            .unwrap();

        let expected = U256::from(100) * U256::from(PRICE_PRECISION);
        assert_eq!(margin, expected);
    }

    #[test]
    fn test_is_reducing() {
        use crate::perpdex::InMemoryStorage;

        let storage = Arc::new(InMemoryStorage::new());
        let config = BlockConfig {
            timestamp: 1000,
            block_number: 1,
        };
        let executor = PerpDexExecutor::new(storage, config);

        // Long position of 100
        let position_size = 100i128;

        // Selling 50 = reducing
        assert!(executor.is_reducing(-50, position_size));

        // Buying 50 = increasing
        assert!(!executor.is_reducing(50, position_size));

        // Short position of -100
        let position_size = -100i128;

        // Buying 50 = reducing
        assert!(executor.is_reducing(50, position_size));

        // Selling 50 = increasing
        assert!(!executor.is_reducing(-50, position_size));
    }
}

