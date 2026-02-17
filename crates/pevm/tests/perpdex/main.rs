//! Integration tests for mixed EVM + PerpDex blocks.

use std::sync::Arc;

use alloy_primitives::{address, Address, TxKind, U256};
use alloy_consensus::{TxLegacy, Transaction};

use pevm::{
    hybrid::{encode_perpdex_tx, identify_vm_type, VmType, PERPDEX_CONTRACT_ADDRESS},
    perpdex::{
        BlockConfig, InMemoryStorage, Operation, OrderType, PerpDexExecutor, PerpDexLocation,
        PerpDexStorage, PerpDexTx, PerpDexValue, Side,
    },
};

#[test]
fn test_identify_transaction_types() {
    // Test EVM transaction
    let evm_tx = TxLegacy {
        chain_id: None,
        nonce: 0,
        gas_price: 1000,
        gas_limit: 21000,
        to: TxKind::Call(address!("0000000000000000000000000000000000000001")),
        value: U256::ZERO,
        input: vec![].into(),
    };
    assert_eq!(identify_vm_type(&evm_tx), VmType::Evm);

    // Test PerpDex transaction
    let perpdex_tx = TxLegacy {
        chain_id: None,
        nonce: 0,
        gas_price: 1000,
        gas_limit: 21000,
        to: TxKind::Call(PERPDEX_CONTRACT_ADDRESS),
        value: U256::ZERO,
        input: vec![].into(),
    };
    assert_eq!(identify_vm_type(&perpdex_tx), VmType::PerpDex);
}

#[test]
fn test_perpdex_executor_basic() {
    // Setup storage with initial balance
    let user = address!("1111111111111111111111111111111111111111");
    let initial_balance = U256::from(10000) * U256::from(10u128.pow(18)); // 10000 USDC

    let storage = InMemoryStorage::with_balances(vec![(user, 0, initial_balance)]);
    let storage_arc = Arc::new(storage);

    let config = BlockConfig {
        timestamp: 1000,
        block_number: 1,
    };

    let executor = PerpDexExecutor::new(storage_arc.clone(), config);

    // Create a simple order transaction
    let perpdex_tx = PerpDexTx {
        user,
        nonce: 0,
        gas_limit: 100000,
        gas_price: U256::from(1000u64),
        signature: None,
        operations: vec![Operation::PlaceOrder {
            market_id: 1,
            order_type: OrderType::Limit,
            side: Side::Long,
            price: U256::from(1000) * U256::from(10u128.pow(18)), // $1000
            size: U256::from(1) * U256::from(10u128.pow(18)),     // 1.0 BTC
            leverage: 10,
            post_only: true,
            reduce_only: false,
            expires_at: None,
        }],
    };

    // Execute transaction (simplified for Phase 3 - not using full MvMemory yet)
    use pevm::perpdex::executor_traits::{MvMemory, TxVersion, VmExecutor};

    let tx_version = TxVersion {
        tx_idx: 0,
        tx_incarnation: 0,
    };

    let mv_memory = MvMemory; // Placeholder

    let result = executor.execute(&tx_version, &perpdex_tx, &*storage_arc, &mv_memory);

    // Verify execution succeeded
    match result {
        pevm::perpdex::executor_traits::VmExecutionResult::Success {
            result,
            ..
        } => {
            println!("Gas used: {}", result.gas_used);
            println!("Operations: {:?}", result.operations_executed);
            assert_eq!(result.operations_executed.len(), 1);
        }
        pevm::perpdex::executor_traits::VmExecutionResult::Error(e) => {
            panic!("Execution failed: {:?}", e);
        }
        other => {
            panic!("Unexpected result: {:?}", other);
        }
    }
}

#[test]
fn test_encode_decode_perpdex_tx() {
    let user = address!("2222222222222222222222222222222222222222");

    let original_tx = PerpDexTx {
        user,
        nonce: 42,
        gas_limit: 200000,
        gas_price: U256::from(5000u64),
        signature: None,
        operations: vec![
            Operation::PlaceOrder {
                market_id: 1,
                order_type: OrderType::Market,
                side: Side::Long,
                price: U256::from(1500) * U256::from(10u128.pow(18)),
                size: U256::from(2) * U256::from(10u128.pow(18)),
                leverage: 5,
                post_only: false,
                reduce_only: false,
                expires_at: Some(2000),
            },
            Operation::CancelAll { market_id: Some(1) },
        ],
    };

    let encoded = encode_perpdex_tx(&original_tx);

    // Verify encoded data structure
    assert!(encoded.len() >= 44); // Minimum header size

    // Verify user address
    assert_eq!(&encoded[0..20], user.as_slice());

    // Verify nonce
    let decoded_nonce = u64::from_be_bytes(encoded[20..28].try_into().unwrap());
    assert_eq!(decoded_nonce, 42);

    println!("Successfully encoded {} operations into {} bytes",
             original_tx.operations.len(),
             encoded.len());
}

#[test]
fn test_perpdex_market_order() {
    let user = address!("3333333333333333333333333333333333333333");
    let initial_balance = U256::from(100000) * U256::from(10u128.pow(18)); // 100k USDC

    let storage = InMemoryStorage::with_balances(vec![(user, 0, initial_balance)]);
    let storage_arc = Arc::new(storage);

    let config = BlockConfig {
        timestamp: 2000,
        block_number: 2,
    };

    let executor = PerpDexExecutor::new(storage_arc.clone(), config);

    let perpdex_tx = PerpDexTx {
        user,
        nonce: 0,
        gas_limit: 100000,
        gas_price: U256::from(1000u64),
        signature: None,
        operations: vec![Operation::PlaceOrder {
            market_id: 1,
            order_type: OrderType::Market,
            side: Side::Long,
            price: U256::from(1000) * U256::from(10u128.pow(18)),
            size: U256::from(1) * U256::from(10u128.pow(18)),
            leverage: 10,
            post_only: false,
            reduce_only: false,
            expires_at: None,
        }],
    };

    use pevm::perpdex::executor_traits::{MvMemory, TxVersion, VmExecutor};

    let tx_version = TxVersion {
        tx_idx: 0,
        tx_incarnation: 0,
    };

    let mv_memory = MvMemory;

    let result = executor.execute(&tx_version, &perpdex_tx, &*storage_arc, &mv_memory);

    match result {
        pevm::perpdex::executor_traits::VmExecutionResult::Success {
            result,
            write_set,
            ..
        } => {
            println!("Market order executed:");
            println!("  Gas used: {}", result.gas_used);
            println!("  Operations: {} completed", result.operations_executed.len());
            println!("  Write set size: {}", write_set.iter().count());

            // Market orders should fill immediately
            assert_eq!(result.operations_executed.len(), 1);
        }
        pevm::perpdex::executor_traits::VmExecutionResult::Error(e) => {
            panic!("Market order execution failed: {:?}", e);
        }
        other => {
            panic!("Unexpected result: {:?}", other);
        }
    }
}

#[test]
fn test_perpdex_batch_operations() {
    let user = address!("4444444444444444444444444444444444444444");
    let initial_balance = U256::from(50000) * U256::from(10u128.pow(18));

    let storage = InMemoryStorage::with_balances(vec![(user, 0, initial_balance)]);
    let storage_arc = Arc::new(storage);

    let config = BlockConfig {
        timestamp: 3000,
        block_number: 3,
    };

    let executor = PerpDexExecutor::new(storage_arc.clone(), config);

    // Batch operations: place order + set leverage + adjust margin
    let perpdex_tx = PerpDexTx {
        user,
        nonce: 0,
        gas_limit: 300000,
        gas_price: U256::from(1000u64),
        signature: None,
        operations: vec![
            Operation::PlaceOrder {
                market_id: 1,
                order_type: OrderType::Limit,
                side: Side::Long,
                price: U256::from(950) * U256::from(10u128.pow(18)),
                size: U256::from(1) * U256::from(10u128.pow(18)),
                leverage: 5,
                post_only: true,
                reduce_only: false,
                expires_at: None,
            },
            Operation::SetLeverage {
                market_id: 1,
                leverage: 10,
            },
        ],
    };

    use pevm::perpdex::executor_traits::{MvMemory, TxVersion, VmExecutor};

    let tx_version = TxVersion {
        tx_idx: 0,
        tx_incarnation: 0,
    };

    let mv_memory = MvMemory;

    let result = executor.execute(&tx_version, &perpdex_tx, &*storage_arc, &mv_memory);

    match result {
        pevm::perpdex::executor_traits::VmExecutionResult::Success {
            result,
            ..
        } => {
            println!("Batch operations completed:");
            println!("  Total gas used: {}", result.gas_used);
            println!("  Operations executed: {}", result.operations_executed.len());

            // Should execute all operations
            assert_eq!(result.operations_executed.len(), 2);
        }
        pevm::perpdex::executor_traits::VmExecutionResult::Error(e) => {
            panic!("Batch operations failed: {:?}", e);
        }
        other => {
            panic!("Unexpected result: {:?}", other);
        }
    }
}
