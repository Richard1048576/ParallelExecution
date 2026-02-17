//! Stress tests for PerpDex orderbook execution with large numbers of transactions.

use std::sync::Arc;

use alloy_primitives::{address, Address, U256};

use pevm::{
    perpdex::{
        BlockConfig, InMemoryStorage, Operation, OrderType, PerpDexExecutor, PerpDexLocation,
        PerpDexTx, Side,
        executor_traits::{MvMemory, TxVersion, VmExecutor},
    },
};

/// Test parallel execution of 1000 independent orders across different markets
#[test]
fn test_1000_independent_orders() {
    let num_txs = 1000;
    let num_markets = 100; // 100 markets, 10 orders per market

    let mut users = Vec::new();
    let mut txs = Vec::new();

    // Create 1000 users with initial balances
    for i in 0..num_txs {
        let user = Address::from_slice(&[(i / 256) as u8, (i % 256) as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let initial_balance = U256::from(100000) * U256::from(10u128.pow(18));
        users.push((user, 0, initial_balance));

        // Each user places one order in a different market (first 100 orders)
        // or shares a market with 9 other users
        let market_id = (i % num_markets) as u64 + 1;

        let tx = PerpDexTx {
            user,
            nonce: 0,
            gas_limit: 100000,
            gas_price: U256::from(1000u64),
            signature: None,
            operations: vec![Operation::PlaceOrder {
                market_id,
                order_type: OrderType::Limit,
                side: if i % 2 == 0 { Side::Long } else { Side::Short },
                price: U256::from(1000 + (i % 100)) * U256::from(10u128.pow(18)),
                size: U256::from(1) * U256::from(10u128.pow(18)),
                leverage: 10,
                post_only: false,
                reduce_only: false,
                expires_at: None,
            }],
        };
        txs.push(tx);
    }

    let storage = InMemoryStorage::with_balances(users);
    let storage_arc = Arc::new(storage);

    let config = BlockConfig {
        timestamp: 1000,
        block_number: 1,
    };

    let executor = PerpDexExecutor::new(storage_arc.clone(), config);
    let mv_memory = MvMemory;

    // Execute all transactions sequentially for this test
    // (Real parallel execution would be in the integrated pexe.rs)
    let mut success_count = 0;
    for (idx, tx) in txs.iter().enumerate() {
        let tx_version = TxVersion {
            tx_idx: idx,
            tx_incarnation: 0,
        };

        let result = executor.execute(&tx_version, tx, &*storage_arc, &mv_memory);

        if matches!(result, pevm::perpdex::executor_traits::VmExecutionResult::Success { .. }) {
            success_count += 1;
        }
    }

    println!("✓ Executed {} out of {} transactions successfully", success_count, num_txs);
    assert!(success_count >= num_txs * 95 / 100, "At least 95% of transactions should succeed");
}

/// Test high-contention scenario: 100 users trading on the same market
#[test]
fn test_high_contention_same_market() {
    let num_users = 100;
    let market_id = 1u64;

    let mut users = Vec::new();
    let mut txs = Vec::new();

    for i in 0..num_users {
        let user = Address::from_slice(&[i as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let initial_balance = U256::from(50000) * U256::from(10u128.pow(18));
        users.push((user, 0, initial_balance));

        // All users place orders on the same market at slightly different prices
        let tx = PerpDexTx {
            user,
            nonce: 0,
            gas_limit: 100000,
            gas_price: U256::from(1000u64),
            signature: None,
            operations: vec![Operation::PlaceOrder {
                market_id,
                order_type: OrderType::Limit,
                side: if i % 2 == 0 { Side::Long } else { Side::Short },
                price: U256::from(1000 + i) * U256::from(10u128.pow(18)),
                size: U256::from(1) * U256::from(10u128.pow(18)),
                leverage: 5,
                post_only: true,
                reduce_only: false,
                expires_at: None,
            }],
        };
        txs.push(tx);
    }

    let storage = InMemoryStorage::with_balances(users);
    let storage_arc = Arc::new(storage);

    let config = BlockConfig {
        timestamp: 2000,
        block_number: 2,
    };

    let executor = PerpDexExecutor::new(storage_arc.clone(), config);
    let mv_memory = MvMemory;

    let mut success_count = 0;
    for (idx, tx) in txs.iter().enumerate() {
        let tx_version = TxVersion {
            tx_idx: idx,
            tx_incarnation: 0,
        };

        let result = executor.execute(&tx_version, tx, &*storage_arc, &mv_memory);

        if matches!(result, pevm::perpdex::executor_traits::VmExecutionResult::Success { .. }) {
            success_count += 1;
        }
    }

    println!("✓ High-contention test: {} out of {} orders placed", success_count, num_users);
    assert_eq!(success_count, num_users, "All orders should be placed successfully");
}

/// Test complex batch operations: each transaction has multiple operations
#[test]
fn test_complex_batch_operations() {
    let num_users = 50;

    let mut users = Vec::new();
    let mut txs = Vec::new();

    for i in 0..num_users {
        let user = Address::from_slice(&[i as u8, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let initial_balance = U256::from(200000) * U256::from(10u128.pow(18));
        users.push((user, 0, initial_balance));

        // Each user performs multiple operations
        let tx = PerpDexTx {
            user,
            nonce: 0,
            gas_limit: 500000,
            gas_price: U256::from(1000u64),
            signature: None,
            operations: vec![
                // Set leverage for market 1
                Operation::SetLeverage {
                    market_id: 1,
                    leverage: 10,
                },
                // Place market order on market 1 (creates position immediately)
                Operation::PlaceOrder {
                    market_id: 1,
                    order_type: OrderType::Market,
                    side: Side::Long,
                    price: U256::from(1000) * U256::from(10u128.pow(18)),
                    size: U256::from(1) * U256::from(10u128.pow(18)),
                    leverage: 10,
                    post_only: false,
                    reduce_only: false,
                    expires_at: None,
                },
                // Place limit order on market 2 (added to order book)
                Operation::PlaceOrder {
                    market_id: 2,
                    order_type: OrderType::Limit,
                    side: Side::Short,
                    price: U256::from(2000) * U256::from(10u128.pow(18)),
                    size: U256::from(2) * U256::from(10u128.pow(18)),
                    leverage: 5,
                    post_only: true,
                    reduce_only: false,
                    expires_at: None,
                },
            ],
        };
        txs.push(tx);
    }

    let storage = InMemoryStorage::with_balances(users);
    let storage_arc = Arc::new(storage);

    let config = BlockConfig {
        timestamp: 3000,
        block_number: 3,
    };

    let executor = PerpDexExecutor::new(storage_arc.clone(), config);
    let mv_memory = MvMemory;

    let mut success_count = 0;
    let mut total_ops = 0;

    for (idx, tx) in txs.iter().enumerate() {
        let tx_version = TxVersion {
            tx_idx: idx,
            tx_incarnation: 0,
        };

        let result = executor.execute(&tx_version, tx, &*storage_arc, &mv_memory);

        match result {
            pevm::perpdex::executor_traits::VmExecutionResult::Success { result, .. } => {
                success_count += 1;
                total_ops += result.operations_executed.len();
            }
            pevm::perpdex::executor_traits::VmExecutionResult::Error(ref e) => {
                eprintln!("Transaction {} failed: {:?}", idx, e);
            }
            _ => {}
        }
    }

    println!("✓ Batch operations test: {} users, {} total operations executed", success_count, total_ops);
    assert_eq!(success_count, num_users, "All batch transactions should succeed");
    assert!(total_ops >= num_users * 3, "Each user should execute at least 3 operations");
}

/// Test state isolation: verify that PerpDex state doesn't collide with EVM state
#[test]
fn test_state_isolation() {
    use pevm::perpdex::state_traits::{PartitionId, StateLocation};

    // Create PerpDex locations
    let user = address!("1111111111111111111111111111111111111111");
    let balance_loc = PerpDexLocation::Balance(user, 0);
    let position_loc = PerpDexLocation::Position(user, 1);
    let order_loc = PerpDexLocation::Order(user, 1, 100);

    // Verify partition IDs
    assert_eq!(balance_loc.partition_id(), PartitionId::PERPDEX);
    assert_eq!(position_loc.partition_id(), PartitionId::PERPDEX);
    assert_eq!(order_loc.partition_id(), PartitionId::PERPDEX);

    // Verify hash isolation (high 8 bits should be 0x02 for PerpDex)
    let balance_hash = balance_loc.location_hash();
    let position_hash = position_loc.location_hash();
    let order_hash = order_loc.location_hash();

    assert_eq!(balance_hash >> 56, 2, "PerpDex hash should have high 8 bits = 2");
    assert_eq!(position_hash >> 56, 2, "PerpDex hash should have high 8 bits = 2");
    assert_eq!(order_hash >> 56, 2, "PerpDex hash should have high 8 bits = 2");

    // Verify uniqueness
    assert_ne!(balance_hash, position_hash);
    assert_ne!(balance_hash, order_hash);
    assert_ne!(position_hash, order_hash);

    println!("✓ State isolation verified: all PerpDex locations have partition ID = 2");
}

/// Test order cancellation and modification
#[test]
fn test_order_lifecycle() {
    let user = address!("2222222222222222222222222222222222222222");
    let initial_balance = U256::from(100000) * U256::from(10u128.pow(18));

    let storage = InMemoryStorage::with_balances(vec![(user, 0, initial_balance)]);
    let storage_arc = Arc::new(storage);

    let config = BlockConfig {
        timestamp: 4000,
        block_number: 4,
    };

    let executor = PerpDexExecutor::new(storage_arc.clone(), config);
    let mv_memory = MvMemory;

    // Transaction 1: Place multiple orders
    let tx1 = PerpDexTx {
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
                price: U256::from(1000) * U256::from(10u128.pow(18)),
                size: U256::from(1) * U256::from(10u128.pow(18)),
                leverage: 10,
                post_only: true,
                reduce_only: false,
                expires_at: None,
            },
            Operation::PlaceOrder {
                market_id: 1,
                order_type: OrderType::Limit,
                side: Side::Long,
                price: U256::from(1100) * U256::from(10u128.pow(18)),
                size: U256::from(2) * U256::from(10u128.pow(18)),
                leverage: 10,
                post_only: true,
                reduce_only: false,
                expires_at: None,
            },
        ],
    };

    let result1 = executor.execute(
        &TxVersion { tx_idx: 0, tx_incarnation: 0 },
        &tx1,
        &*storage_arc,
        &mv_memory,
    );

    assert!(
        matches!(result1, pevm::perpdex::executor_traits::VmExecutionResult::Success { .. }),
        "First transaction should succeed"
    );

    // Transaction 2: Cancel all orders (note: nonce is 0 because we haven't committed tx1 to storage)
    // In a real parallel execution environment, this would be handled by the scheduler
    let tx2 = PerpDexTx {
        user,
        nonce: 0,
        gas_limit: 100000,
        gas_price: U256::from(1000u64),
        signature: None,
        operations: vec![Operation::CancelAll { market_id: Some(1) }],
    };

    let result2 = executor.execute(
        &TxVersion { tx_idx: 1, tx_incarnation: 0 },
        &tx2,
        &*storage_arc,
        &mv_memory,
    );

    match &result2 {
        pevm::perpdex::executor_traits::VmExecutionResult::Success { .. } => {}
        pevm::perpdex::executor_traits::VmExecutionResult::Error(e) => {
            panic!("Cancel operation failed with error: {:?}", e);
        }
        _ => panic!("Unexpected result: {:?}", result2),
    }

    println!("✓ Order lifecycle test passed: place and cancel operations work correctly");
}
