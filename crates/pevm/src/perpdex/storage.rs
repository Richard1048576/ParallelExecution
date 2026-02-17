//! PerpDex storage abstraction.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use alloy_primitives::{Address, U256};

use super::state::{PerpDexLocation, PerpDexValue};

/// Error type for storage operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum StorageError {
    /// Key not found
    #[error("Key not found: {0:?}")]
    NotFound(PerpDexLocation),

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),
}

/// Storage backend for PerpDex state.
///
/// This trait abstracts over different storage implementations
/// (in-memory, RPC, database, etc.).
pub trait PerpDexStorage: Send + Sync {
    /// Read a value from storage
    fn get(&self, location: &PerpDexLocation) -> Result<Option<PerpDexValue>, StorageError>;

    /// Write a value to storage
    fn set(&self, location: PerpDexLocation, value: PerpDexValue) -> Result<(), StorageError>;

    /// Delete a value from storage
    fn delete(&self, location: &PerpDexLocation) -> Result<(), StorageError>;

    /// Get all order IDs for a user in a specific market
    fn get_user_orders(&self, user: Address, market_id: u64) -> Result<Vec<u64>, StorageError>;

    /// Get all order IDs for a user across all markets
    fn get_all_user_orders(&self, user: Address) -> Result<Vec<u64>, StorageError>;
}

/// In-memory storage implementation for testing and development.
#[derive(Debug, Clone, Default)]
pub struct InMemoryStorage {
    data: Arc<RwLock<HashMap<PerpDexLocation, PerpDexValue>>>,
    // Track user orders for efficient lookup
    user_orders: Arc<RwLock<HashMap<(Address, u64), Vec<u64>>>>,
}

impl InMemoryStorage {
    /// Create a new empty in-memory storage
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            user_orders: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create storage with initial balances
    pub fn with_balances(balances: Vec<(Address, u16, U256)>) -> Self {
        let storage = Self::new();
        let mut data = storage.data.write().unwrap();
        for (user, asset_id, balance) in balances {
            data.insert(
                PerpDexLocation::Balance(user, asset_id),
                PerpDexValue::Balance(balance),
            );
        }
        drop(data);
        storage
    }

    /// Add an order to the tracking index
    fn add_order_index(&self, user: Address, market_id: u64, order_id: u64) {
        let mut orders = self.user_orders.write().unwrap();
        orders
            .entry((user, market_id))
            .or_insert_with(Vec::new)
            .push(order_id);
    }

    /// Remove an order from the tracking index
    fn remove_order_index(&self, user: Address, market_id: u64, order_id: u64) {
        let mut orders = self.user_orders.write().unwrap();
        if let Some(order_list) = orders.get_mut(&(user, market_id)) {
            order_list.retain(|&id| id != order_id);
        }
    }
}

impl PerpDexStorage for InMemoryStorage {
    fn get(&self, location: &PerpDexLocation) -> Result<Option<PerpDexValue>, StorageError> {
        let data = self.data.read().unwrap();
        Ok(data.get(location).cloned())
    }

    fn set(&self, location: PerpDexLocation, value: PerpDexValue) -> Result<(), StorageError> {
        // Track order additions
        if let PerpDexLocation::Order(user, market_id, order_id) = &location {
            if !matches!(value, PerpDexValue::Deleted) {
                self.add_order_index(*user, *market_id, *order_id);
            }
        }

        let mut data = self.data.write().unwrap();
        data.insert(location, value);
        Ok(())
    }

    fn delete(&self, location: &PerpDexLocation) -> Result<(), StorageError> {
        // Track order deletions
        if let PerpDexLocation::Order(user, market_id, order_id) = location {
            self.remove_order_index(*user, *market_id, *order_id);
        }

        let mut data = self.data.write().unwrap();
        data.remove(location);
        Ok(())
    }

    fn get_user_orders(&self, user: Address, market_id: u64) -> Result<Vec<u64>, StorageError> {
        let orders = self.user_orders.read().unwrap();
        Ok(orders.get(&(user, market_id)).cloned().unwrap_or_default())
    }

    fn get_all_user_orders(&self, user: Address) -> Result<Vec<u64>, StorageError> {
        let orders = self.user_orders.read().unwrap();
        let mut all_orders = Vec::new();
        for ((addr, _), order_ids) in orders.iter() {
            if *addr == user {
                all_orders.extend_from_slice(order_ids);
            }
        }
        Ok(all_orders)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_in_memory_storage_basic() {
        let storage = InMemoryStorage::new();
        let addr = address!("0000000000000000000000000000000000000001");
        let loc = PerpDexLocation::Balance(addr, 0);

        // Initially empty
        assert!(storage.get(&loc).unwrap().is_none());

        // Set value
        storage.set(loc.clone(), PerpDexValue::Balance(U256::from(1000))).unwrap();

        // Read back
        let value = storage.get(&loc).unwrap().unwrap();
        if let PerpDexValue::Balance(bal) = value {
            assert_eq!(bal, U256::from(1000));
        } else {
            panic!("Expected Balance");
        }

        // Delete
        storage.delete(&loc).unwrap();
        assert!(storage.get(&loc).unwrap().is_none());
    }

    #[test]
    fn test_with_balances() {
        let addr1 = address!("0000000000000000000000000000000000000001");
        let addr2 = address!("0000000000000000000000000000000000000002");

        let storage = InMemoryStorage::with_balances(vec![
            (addr1, 0, U256::from(1000)),
            (addr2, 0, U256::from(2000)),
        ]);

        let bal1 = storage.get(&PerpDexLocation::Balance(addr1, 0)).unwrap().unwrap();
        let bal2 = storage.get(&PerpDexLocation::Balance(addr2, 0)).unwrap().unwrap();

        if let PerpDexValue::Balance(b1) = bal1 {
            assert_eq!(b1, U256::from(1000));
        } else {
            panic!("Expected Balance");
        }

        if let PerpDexValue::Balance(b2) = bal2 {
            assert_eq!(b2, U256::from(2000));
        } else {
            panic!("Expected Balance");
        }
    }

    #[test]
    fn test_order_tracking() {
        let storage = InMemoryStorage::new();
        let addr = address!("0000000000000000000000000000000000000001");
        let market_id = 1u64;

        // Initially no orders
        assert!(storage.get_user_orders(addr, market_id).unwrap().is_empty());

        // Add order
        let order_loc = PerpDexLocation::Order(addr, market_id, 100);
        storage.set(order_loc.clone(), PerpDexValue::empty_position()).unwrap();

        // Should be tracked
        let orders = storage.get_user_orders(addr, market_id).unwrap();
        assert_eq!(orders, vec![100]);

        // Delete order
        storage.delete(&order_loc).unwrap();
        assert!(storage.get_user_orders(addr, market_id).unwrap().is_empty());
    }
}
