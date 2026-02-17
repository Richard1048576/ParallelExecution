//! Generic state location and value abstractions for multi-VM parallel execution.

use std::any::TypeId;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use dashmap::DashMap;

/// Trait for identifying state locations across different VM types.
///
/// Each VM type (EVM, WASM, PerpDex, etc.) implements this trait to define
/// how its state locations are hashed and partitioned.
pub trait StateLocation: Clone + Debug + PartialEq + Eq + Hash + Send + Sync + 'static {
    /// Compute deterministic hash for the multi-version data structure.
    ///
    /// This hash is mixed with the partition ID to ensure zero collision
    /// probability across different VM types.
    fn location_hash(&self) -> u64;

    /// Get the partition ID for this location.
    ///
    /// This determines which VM's state space this location belongs to,
    /// ensuring isolation between different execution models.
    fn partition_id(&self) -> PartitionId;
}

/// Trait for values stored at state locations.
///
/// Different VM types can define their own value types (e.g., EVM account data,
/// orderbook entries, WASM memory pages) that implement this trait.
pub trait StateValue: Clone + Debug + Send + Sync + 'static {
    /// Whether this value is lazily evaluated (deferred computation).
    ///
    /// Lazy values are computed at block finalization rather than during
    /// execution to avoid creating artificial dependencies. For example,
    /// EVM uses lazy updates for beneficiary gas payments.
    ///
    /// Default: `false` (strict evaluation).
    fn is_lazy(&self) -> bool {
        false
    }

    /// Whether this value represents deleted/cleared state.
    ///
    /// Used for operations like EVM's SELFDESTRUCT or orderbook cancel.
    ///
    /// Default: `false` (value exists).
    fn is_deleted(&self) -> bool {
        false
    }
}

/// VM partition identifier.
///
/// Partitions provide isolated state spaces for different execution models.
/// The partition ID is mixed into location hashes (using the high 8 bits)
/// to guarantee zero collision probability.
///
/// This allows up to 256 distinct VM types to coexist in the same parallel
/// execution framework.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartitionId(pub u8);

impl PartitionId {
    /// EVM partition (Ethereum Virtual Machine)
    pub const EVM: Self = Self(0);

    /// WASM partition (WebAssembly)
    pub const WASM: Self = Self(1);

    /// PerpDex partition (Perpetual DEX / Orderbook)
    pub const PERPDEX: Self = Self(2);

    /// Mix partition ID into a base hash to create a globally unique hash.
    ///
    /// # Hash Space Design
    ///
    /// ```text
    /// 64-bit hash layout:
    /// ┌────────┬────────────────────────────────────────────────────┐
    /// │ 8 bits │                    56 bits                         │
    /// │Partition│                  Base Hash                        │
    /// └────────┴────────────────────────────────────────────────────┘
    /// ```
    ///
    /// - High 8 bits: Partition ID (0-255)
    /// - Low 56 bits: Base hash from location-specific data
    ///
    /// This guarantees that `EVM::Basic(addr)` and `PerpDex::Balance(addr)`
    /// will never collide even if they use the same address.
    ///
    /// # Examples
    ///
    /// ```
    /// # use pevm::state::PartitionId;
    /// let evm_hash = PartitionId::EVM.mix_hash(0x1234_5678_9ABC_DEF0);
    /// let perp_hash = PartitionId::PERPDEX.mix_hash(0x1234_5678_9ABC_DEF0);
    ///
    /// assert_ne!(evm_hash, perp_hash);
    /// assert_eq!(evm_hash >> 56, 0);      // EVM partition
    /// assert_eq!(perp_hash >> 56, 2);     // PerpDex partition
    /// ```
    #[inline(always)]
    pub fn mix_hash(&self, base_hash: u64) -> u64 {
        // Clear high 8 bits of base hash, then set to partition ID
        ((self.0 as u64) << 56) | (base_hash & 0x00FF_FFFF_FFFF_FFFF)
    }

    /// Extract partition ID from a mixed hash.
    ///
    /// This is the inverse of `mix_hash` for debugging and validation.
    #[inline(always)]
    pub fn from_hash(mixed_hash: u64) -> Self {
        Self((mixed_hash >> 56) as u8)
    }
}

/// Type-erased storage for VM-specific metadata.
///
/// Different VMs may need to store additional data alongside the multi-version
/// memory. For example, EVM stores newly deployed bytecodes that need to be
/// shared across transactions.
///
/// This structure provides type-safe storage and retrieval of VM-specific data
/// without requiring the core parallel execution framework to know about these
/// types.
///
/// # Examples
///
/// ```
/// # use pevm::state::VmMetadata;
/// # use std::collections::HashMap;
/// # use alloy_primitives::{B256, Bytecode};
/// let metadata = VmMetadata::new();
///
/// // EVM stores bytecodes
/// let bytecodes = metadata.get_or_insert::<HashMap<B256, Bytecode>>();
/// // bytecodes.insert(...);
///
/// // Later retrieval
/// let same_bytecodes = metadata.get_or_insert::<HashMap<B256, Bytecode>>();
/// assert!(Arc::ptr_eq(&bytecodes, &same_bytecodes));
/// ```
#[derive(Debug, Default)]
pub struct VmMetadata {
    inner: DashMap<TypeId, Arc<dyn std::any::Any + Send + Sync>>,
}

impl VmMetadata {
    /// Create a new empty metadata store.
    pub fn new() -> Self {
        Self {
            inner: DashMap::new(),
        }
    }

    /// Get or insert VM-specific metadata of type `T`.
    ///
    /// If no value of type `T` exists, creates one using `T::default()`.
    /// Returns an `Arc<T>` that can be safely shared across threads.
    ///
    /// # Type Safety
    ///
    /// Each type `T` has its own isolated storage slot. Different VMs
    /// can use the same metadata store without conflicts.
    pub fn get_or_insert<T>(&self) -> Arc<T>
    where
        T: Default + Send + Sync + 'static,
    {
        self.inner
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Arc::new(T::default()) as Arc<dyn std::any::Any + Send + Sync>)
            .clone()
            .downcast::<T>()
            .expect("TypeId ensures correct type")
    }

    /// Get existing metadata of type `T`, if it exists.
    pub fn get<T>(&self) -> Option<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.inner
            .get(&TypeId::of::<T>())
            .and_then(|entry| entry.clone().downcast::<T>().ok())
    }

    /// Insert metadata of type `T`, replacing any existing value.
    pub fn insert<T>(&self, value: T)
    where
        T: Send + Sync + 'static,
    {
        self.inner
            .insert(TypeId::of::<T>(), Arc::new(value) as Arc<dyn std::any::Any + Send + Sync>);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_hash_mixing() {
        let base_hash = 0x1234_5678_9ABC_DEF0u64;

        let evm_hash = PartitionId::EVM.mix_hash(base_hash);
        let wasm_hash = PartitionId::WASM.mix_hash(base_hash);
        let perp_hash = PartitionId::PERPDEX.mix_hash(base_hash);

        // All should be different
        assert_ne!(evm_hash, wasm_hash);
        assert_ne!(evm_hash, perp_hash);
        assert_ne!(wasm_hash, perp_hash);

        // Partition IDs should be in high byte
        assert_eq!(evm_hash >> 56, 0);
        assert_eq!(wasm_hash >> 56, 1);
        assert_eq!(perp_hash >> 56, 2);

        // Low 56 bits should be preserved
        assert_eq!(evm_hash & 0x00FF_FFFF_FFFF_FFFF, base_hash & 0x00FF_FFFF_FFFF_FFFF);
    }

    #[test]
    fn test_partition_from_hash() {
        let mixed_evm = PartitionId::EVM.mix_hash(0x1234);
        let mixed_perp = PartitionId::PERPDEX.mix_hash(0x5678);

        assert_eq!(PartitionId::from_hash(mixed_evm), PartitionId::EVM);
        assert_eq!(PartitionId::from_hash(mixed_perp), PartitionId::PERPDEX);
    }

    #[test]
    fn test_vm_metadata_isolation() {
        let metadata = VmMetadata::new();

        // Insert different types
        metadata.insert(42u32);
        metadata.insert("hello".to_string());

        // Retrieve them
        let num: Arc<u32> = metadata.get().unwrap();
        let text: Arc<String> = metadata.get().unwrap();

        assert_eq!(*num, 42);
        assert_eq!(&**text, "hello");
    }

    #[test]
    fn test_vm_metadata_get_or_insert() {
        use std::collections::HashMap;

        let metadata = VmMetadata::new();

        let map1 = metadata.get_or_insert::<HashMap<u64, u64>>();
        let map2 = metadata.get_or_insert::<HashMap<u64, u64>>();

        // Should return the same Arc
        assert!(Arc::ptr_eq(&map1, &map2));
    }
}
