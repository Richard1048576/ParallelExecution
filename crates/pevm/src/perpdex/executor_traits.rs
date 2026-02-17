//! Generic VM executor trait for parallel execution.

use super::state_traits::{StateLocation, StateValue};
use hashbrown::HashMap;
use smallvec::SmallVec;

// Type aliases for perpdex module
pub type TxIdx = usize;
pub type TxIncarnation = usize;

#[derive(Clone, Debug, PartialEq)]
pub struct TxVersion {
    pub tx_idx: TxIdx,
    pub tx_incarnation: TxIncarnation,
}

#[derive(Debug, PartialEq, Clone)]
pub enum ReadOrigin {
    MvMemory(TxVersion),
    Storage,
}

pub type ReadOrigins = SmallVec<[ReadOrigin; 1]>;

/// Generic read set for tracking transaction reads
#[derive(Debug, Clone)]
pub struct ReadSet<L: StateLocation> {
    pub(crate) data: HashMap<u64, ReadOrigins>,
    pub(crate) locations: SmallVec<[L; 8]>,
}

impl<L: StateLocation> Default for ReadSet<L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L: StateLocation> ReadSet<L> {
    pub fn new() -> Self {
        Self {
            data: HashMap::default(),
            locations: SmallVec::new(),
        }
    }

    pub fn insert(&mut self, location: L, origin: ReadOrigin) {
        let hash = location.location_hash();
        self.data.entry(hash).or_default().push(origin);
        if !self.locations.iter().any(|l| l.location_hash() == hash) {
            self.locations.push(location);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Generic write set for tracking transaction writes
#[derive(Debug, Clone)]
pub struct WriteSet<L: StateLocation, V: StateValue> {
    pub(crate) data: SmallVec<[(u64, L, V); 3]>,
}

impl<L: StateLocation, V: StateValue> Default for WriteSet<L, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L: StateLocation, V: StateValue> WriteSet<L, V> {
    pub fn new() -> Self {
        Self {
            data: SmallVec::new(),
        }
    }

    pub fn push(&mut self, location: L, value: V) {
        let hash = location.location_hash();
        self.data.push((hash, location, value));
    }

    pub fn iter(&self) -> impl Iterator<Item = &(u64, L, V)> {
        self.data.iter()
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FinishExecFlags: u8 {
        const NeedValidation = 0;
        const WroteNewLocation = 1;
    }
}

/// Placeholder for multi-version memory in Phase 3.
/// In production, this would be the actual MvMemory structure for parallel execution.
#[derive(Debug)]
pub struct MvMemory;


/// Result of executing a transaction in a VM.
///
/// This enum represents all possible outcomes when attempting to execute
/// a transaction in the parallel execution framework.
#[derive(Debug)]
pub enum VmExecutionResult<L: StateLocation, V: StateValue, R> {
    /// Transaction executed successfully.
    Success {
        /// Set of locations read during execution, with version tracking.
        read_set: ReadSet<L>,
        /// Set of locations written during execution.
        write_set: WriteSet<L, V>,
        /// VM-specific execution result (e.g., gas used, return data).
        result: R,
        /// Flags indicating validation requirements.
        flags: FinishExecFlags,
    },

    /// Execution blocked on an unfinished earlier transaction.
    ///
    /// This occurs when reading a location that was written by transaction
    /// `blocking_tx_idx` but that transaction hasn't been validated yet.
    ///
    /// The scheduler will re-execute this transaction after the blocking
    /// transaction completes.
    Blocked(TxIdx),

    /// Execution should be retried immediately.
    ///
    /// This can occur when a transaction reads an ESTIMATE marker that
    /// was just converted to a real value. Unlike `Blocked`, the blocking
    /// transaction is already complete.
    Retry,

    /// Parallel execution failed, fallback to sequential execution.
    ///
    /// Some transactions may be incompatible with parallel execution
    /// (e.g., excessive conflicts, non-deterministic behavior). When this
    /// occurs, the framework falls back to sequential execution.
    FallbackToSequential,

    /// Execution error occurred.
    Error(Box<dyn std::error::Error + Send + Sync>),
}

/// Generic VM executor for parallel transaction execution.
///
/// This trait abstracts over different virtual machines (EVM, WASM, PerpDex, etc.)
/// allowing them to participate in the same parallel execution framework.
///
/// # Type Parameters
///
/// - `Location`: State location type (e.g., EVM addresses + storage slots)
/// - `Value`: State value type (e.g., EVM account data)
/// - `Transaction`: Transaction format (e.g., EVM TxEnv)
/// - `Result`: Execution result format (e.g., gas used, logs)
/// - `Storage`: Storage backend interface (e.g., RPC, in-memory)
///
/// # Parallel Execution Flow
///
/// 1. **Execution**: `execute()` reads from multi-version memory and storage,
///    produces a read set and write set.
/// 2. **Recording**: Write set is recorded to multi-version memory.
/// 3. **Validation**: Read set is validated against current memory state.
/// 4. **Re-execution**: If validation fails, transaction is re-executed.
///
/// # Example Implementation
///
/// ```ignore
/// impl VmExecutor for EvmExecutor {
///     type Location = EvmLocation;
///     type Value = EvmValue;
///     type Transaction = TxEnv;
///     type Result = PexeTxExecutionResult;
///     type Storage = dyn Storage;
///
///     fn execute(
///         &self,
///         tx_version: &TxVersion,
///         tx: &TxEnv,
///         storage: &Self::Storage,
///         mv_memory: &MvMemory,
///     ) -> VmExecutionResult<EvmLocation, EvmValue, PexeTxExecutionResult> {
///         // Execute EVM transaction, track reads/writes
///         // ...
///     }
/// }
/// ```
pub trait VmExecutor: Send + Sync {
    /// State location type for this VM.
    type Location: StateLocation;

    /// State value type for this VM.
    type Value: StateValue;

    /// Transaction format for this VM.
    type Transaction: Clone + Send + Sync;

    /// Execution result format for this VM.
    type Result: Clone + Send + Sync;

    /// Storage backend interface for this VM.
    type Storage: Send + Sync + ?Sized;

    /// Execute a transaction at the given version.
    ///
    /// # Parameters
    ///
    /// - `tx_version`: Transaction index and incarnation number
    /// - `tx`: The transaction to execute
    /// - `storage`: Read-only access to the pre-block state
    /// - `mv_memory`: Multi-version memory for reading speculative writes
    ///
    /// # Returns
    ///
    /// - `Success`: Execution completed, returns read/write sets and result
    /// - `Blocked`: Execution blocked on unfinished transaction
    /// - `Retry`: Should retry immediately
    /// - `FallbackToSequential`: Parallel execution not possible
    /// - `Error`: Execution error
    ///
    /// # Read Set Tracking
    ///
    /// Implementations must track all state reads in the `read_set`, recording:
    /// - The location read
    /// - Whether it came from multi-version memory (with version) or storage
    ///
    /// This enables validation: checking if the read values are still valid
    /// after other transactions complete.
    ///
    /// # Write Set Construction
    ///
    /// Implementations must collect all state writes in the `write_set`:
    /// - The location written
    /// - The new value (or deletion marker)
    ///
    /// Write sets are optimistically applied to multi-version memory and
    /// validated before being committed.
    ///
    /// # Blocking Behavior
    ///
    /// If execution reads a location that has an ESTIMATE marker (from an
    /// aborted transaction), return `Blocked(blocking_tx_idx)` to avoid
    /// wasting work on potentially incorrect reads.
    ///
    /// The scheduler will automatically re-execute after the blocking
    /// transaction completes.
    fn execute(
        &self,
        tx_version: &TxVersion,
        tx: &Self::Transaction,
        storage: &Self::Storage,
        mv_memory: &MvMemory,
    ) -> VmExecutionResult<Self::Location, Self::Value, Self::Result>;

    /// Get locations that should use lazy evaluation for this transaction.
    ///
    /// Lazy evaluation defers computation of certain values to block
    /// finalization, avoiding artificial dependencies. For example, EVM
    /// uses lazy updates for:
    /// - Beneficiary gas payments (all transactions pay the same account)
    /// - Common recipient addresses (e.g., CEX hot wallets)
    ///
    /// # Default Implementation
    ///
    /// Returns empty vec (no lazy evaluation).
    ///
    /// # Example: EVM Lazy Locations
    ///
    /// ```ignore
    /// fn get_lazy_locations(&self, tx: &TxEnv) -> Vec<EvmLocation> {
    ///     let mut locs = vec![EvmLocation::Basic(tx.caller)];  // sender
    ///     if let TxKind::Call(to) = tx.transact_to {
    ///         locs.push(EvmLocation::Basic(to));  // recipient (if not contract)
    ///     }
    ///     locs
    /// }
    /// ```
    fn get_lazy_locations(&self, _tx: &Self::Transaction) -> Vec<Self::Location> {
        Vec::new()
    }
}
