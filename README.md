# Parallel Execution Engine

A high-performance parallel execution engine built in Rust, designed to parallelize **both EVM transactions and Perpetual DEX (Perp DEX) transactions** for maximum throughput and minimum latency.

## Overview

Blockchain transaction execution has traditionally been sequential, leaving modern multi-core CPUs vastly underutilized. This project builds a unified parallel execution engine that can handle:

- **EVM Transactions**: Standard Ethereum Virtual Machine transactions (transfers, smart contract calls, DeFi operations, etc.) are executed in parallel with automatic dependency detection -- no changes to existing smart contracts required.
- **Perp DEX Transactions**: Perpetual decentralized exchange operations (order matching, position management, liquidations, funding rate updates, etc.) are parallelized with a custom execution model optimized for high-frequency trading workloads.

## Key Features

- **Automatic Parallelism Detection**: The engine dynamically identifies independent transactions and executes them concurrently, without requiring developers to declare state access upfront.
- **Hybrid Execution Model**: Combines optimistic parallel execution (Block-STM based) for EVM transactions with a purpose-built parallel engine for Perp DEX operations.
- **Lazy State Updates**: Implicit reads and writes (e.g., gas payments to the beneficiary, ERC-20 transfers) are lazily evaluated to reduce false dependencies and maximize parallelism.
- **High-Performance Rust Implementation**: Tightly implemented in Rust with native CPU optimizations, purpose-tuned memory allocators, and minimal runtime overhead.

## Architecture

The engine uses a collaborative scheduler with a multi-version data structure to detect state conflicts and re-execute transactions when necessary, ensuring deterministic results identical to sequential execution.

Core components:

- **Scheduler**: Resource-aware task scheduler that assigns transactions to worker threads and manages re-execution upon conflicts.
- **Multi-Version Memory (MvMemory)**: A concurrent data structure that tracks read/write sets for conflict detection across parallel transactions.
- **Perp DEX Module**: A dedicated execution module for perpetual DEX operations, supporting parallel order processing and position management.

## Development

### Prerequisites

- Rust toolchain (edition 2024)
- [cmake](https://cmake.org) (for building the `snmalloc` memory allocator)

### Build

```sh
cargo build
```

### Test

```sh
# Initialize submodules for Ethereum state tests
git submodule update --init

# Run tests (sequential test threads to avoid resource contention)
cargo test --workspace --release -- --test-threads=1
```

### Benchmark

```sh
# EVM transaction benchmarks
cargo bench -p pevm --bench mainnet

# Perp DEX benchmarks
cargo bench -p pevm --bench perpdex
```

## Project Structure

```
crates/pevm/          # Core parallel execution engine
  src/
    perpdex/          # Perp DEX parallel execution module
    chain/            # Chain-specific configurations (Ethereum, OP Stack)
    scheduler.rs      # Parallel task scheduler
    mv_memory.rs      # Multi-version concurrent data structure
    pexe.rs           # Main parallel execution entry point
  tests/
    ethereum/         # Ethereum general state tests
    perpdex/          # Perp DEX execution tests
    erc20/            # ERC-20 parallel execution tests
    uniswap/          # Uniswap swap parallel execution tests
  benches/            # Performance benchmarks
bins/fetch/           # CLI tool for fetching real Ethereum blocks for testing
data/                 # Ethereum mainnet block snapshots for testing
```

## License

See [LICENSE](./LICENSE) for details.
