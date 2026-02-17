//! PerpDex (Perpetual DEX) parallel execution module.
//!
//! This module implements a parallel execution engine for perpetual futures
//! orderbook transactions. It can run alongside EVM transactions in the same
//! block using the partition isolation mechanism.

// Core trait definitions (internal to perpdex until full migration)
pub mod state_traits;
pub mod executor_traits;

pub mod executor;
pub mod state;
pub mod storage;
pub mod types;

// Re-exports for convenience
pub use executor::{BlockConfig, ExecutionError, PerpDexExecutor};
pub use state::{PerpDexLocation, PerpDexValue};
pub use state_traits::{PartitionId, StateLocation, StateValue};
pub use storage::{InMemoryStorage, PerpDexStorage, StorageError};
pub use types::{
    Operation, OperationResult, OrderStatus, OrderType, PerpDexResult, PerpDexTx, Side,
};
