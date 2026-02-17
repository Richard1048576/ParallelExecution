//! Benchmarks for PerpDex orderbook execution.

use std::sync::Arc;

use alloy_primitives::{Address, U256};
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};

use pevm::perpdex::{
    BlockConfig, InMemoryStorage, Operation, OrderType, PerpDexExecutor, PerpDexTx, Side,
    executor_traits::{MvMemory, TxVersion, VmExecutor},
};

fn create_test_user(index: usize) -> Address {
    Address::from_slice(&[
        (index / 256) as u8,
        (index % 256) as u8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ])
}

fn benchmark_single_market_order(c: &mut Criterion) {
    c.bench_function("perpdex_single_market_order", |b| {
        b.iter_batched(
            || {
                let user = create_test_user(0);
                let initial_balance = U256::from(100000) * U256::from(10u128.pow(18));
                let storage = InMemoryStorage::with_balances(vec![(user, 0, initial_balance)]);
                let storage_arc = Arc::new(storage);
                let config = BlockConfig {
                    timestamp: 1000,
                    block_number: 1,
                };
                let executor = PerpDexExecutor::new(storage_arc.clone(), config);

                let tx = PerpDexTx {
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

                (executor, tx, storage_arc)
            },
            |(executor, tx, storage_arc)| {
                let tx_version = TxVersion {
                    tx_idx: 0,
                    tx_incarnation: 0,
                };
                let mv_memory = MvMemory;
                black_box(executor.execute(&tx_version, &tx, &*storage_arc, &mv_memory))
            },
            BatchSize::SmallInput,
        )
    });
}

fn benchmark_single_limit_order(c: &mut Criterion) {
    c.bench_function("perpdex_single_limit_order", |b| {
        b.iter_batched(
            || {
                let user = create_test_user(1);
                let initial_balance = U256::from(100000) * U256::from(10u128.pow(18));
                let storage = InMemoryStorage::with_balances(vec![(user, 0, initial_balance)]);
                let storage_arc = Arc::new(storage);
                let config = BlockConfig {
                    timestamp: 1000,
                    block_number: 1,
                };
                let executor = PerpDexExecutor::new(storage_arc.clone(), config);

                let tx = PerpDexTx {
                    user,
                    nonce: 0,
                    gas_limit: 100000,
                    gas_price: U256::from(1000u64),
                    signature: None,
                    operations: vec![Operation::PlaceOrder {
                        market_id: 1,
                        order_type: OrderType::Limit,
                        side: Side::Long,
                        price: U256::from(1000) * U256::from(10u128.pow(18)),
                        size: U256::from(1) * U256::from(10u128.pow(18)),
                        leverage: 10,
                        post_only: true,
                        reduce_only: false,
                        expires_at: None,
                    }],
                };

                (executor, tx, storage_arc)
            },
            |(executor, tx, storage_arc)| {
                let tx_version = TxVersion {
                    tx_idx: 0,
                    tx_incarnation: 0,
                };
                let mv_memory = MvMemory;
                black_box(executor.execute(&tx_version, &tx, &*storage_arc, &mv_memory))
            },
            BatchSize::SmallInput,
        )
    });
}

fn benchmark_batch_operations(c: &mut Criterion) {
    c.bench_function("perpdex_batch_operations", |b| {
        b.iter_batched(
            || {
                let user = create_test_user(2);
                let initial_balance = U256::from(200000) * U256::from(10u128.pow(18));
                let storage = InMemoryStorage::with_balances(vec![(user, 0, initial_balance)]);
                let storage_arc = Arc::new(storage);
                let config = BlockConfig {
                    timestamp: 1000,
                    block_number: 1,
                };
                let executor = PerpDexExecutor::new(storage_arc.clone(), config);

                let tx = PerpDexTx {
                    user,
                    nonce: 0,
                    gas_limit: 300000,
                    gas_price: U256::from(1000u64),
                    signature: None,
                    operations: vec![
                        Operation::SetLeverage {
                            market_id: 1,
                            leverage: 10,
                        },
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

                (executor, tx, storage_arc)
            },
            |(executor, tx, storage_arc)| {
                let tx_version = TxVersion {
                    tx_idx: 0,
                    tx_incarnation: 0,
                };
                let mv_memory = MvMemory;
                black_box(executor.execute(&tx_version, &tx, &*storage_arc, &mv_memory))
            },
            BatchSize::SmallInput,
        )
    });
}

fn benchmark_many_independent_orders(c: &mut Criterion) {
    c.bench_function("perpdex_100_independent_orders", |b| {
        b.iter_batched(
            || {
                let num_users = 100;
                let mut users = Vec::new();
                let mut txs = Vec::new();

                for i in 0..num_users {
                    let user = create_test_user(i);
                    let initial_balance = U256::from(50000) * U256::from(10u128.pow(18));
                    users.push((user, 0, initial_balance));

                    let tx = PerpDexTx {
                        user,
                        nonce: 0,
                        gas_limit: 100000,
                        gas_price: U256::from(1000u64),
                        signature: None,
                        operations: vec![Operation::PlaceOrder {
                            market_id: (i % 10) as u64 + 1, // 10 different markets
                            order_type: OrderType::Limit,
                            side: if i % 2 == 0 { Side::Long } else { Side::Short },
                            price: U256::from(1000 + i) * U256::from(10u128.pow(18)),
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

                (executor, txs, storage_arc)
            },
            |(executor, txs, storage_arc)| {
                let mv_memory = MvMemory;
                for (idx, tx) in txs.iter().enumerate() {
                    let tx_version = TxVersion {
                        tx_idx: idx,
                        tx_incarnation: 0,
                    };
                    black_box(executor.execute(&tx_version, tx, &*storage_arc, &mv_memory));
                }
            },
            BatchSize::SmallInput,
        )
    });
}

fn benchmark_high_contention(c: &mut Criterion) {
    c.bench_function("perpdex_50_orders_same_market", |b| {
        b.iter_batched(
            || {
                let num_users = 50;
                let mut users = Vec::new();
                let mut txs = Vec::new();

                for i in 0..num_users {
                    let user = create_test_user(i + 1000);
                    let initial_balance = U256::from(50000) * U256::from(10u128.pow(18));
                    users.push((user, 0, initial_balance));

                    let tx = PerpDexTx {
                        user,
                        nonce: 0,
                        gas_limit: 100000,
                        gas_price: U256::from(1000u64),
                        signature: None,
                        operations: vec![Operation::PlaceOrder {
                            market_id: 1, // Same market for all
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
                    timestamp: 1000,
                    block_number: 1,
                };
                let executor = PerpDexExecutor::new(storage_arc.clone(), config);

                (executor, txs, storage_arc)
            },
            |(executor, txs, storage_arc)| {
                let mv_memory = MvMemory;
                for (idx, tx) in txs.iter().enumerate() {
                    let tx_version = TxVersion {
                        tx_idx: idx,
                        tx_incarnation: 0,
                    };
                    black_box(executor.execute(&tx_version, tx, &*storage_arc, &mv_memory));
                }
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(
    benches,
    benchmark_single_market_order,
    benchmark_single_limit_order,
    benchmark_batch_operations,
    benchmark_many_independent_orders,
    benchmark_high_contention
);
criterion_main!(benches);
