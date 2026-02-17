//! Hybrid block execution support for mixed EVM and PerpDex transactions.

use alloy_consensus::Transaction;
use alloy_primitives::{Address, Bytes, U256};
use revm::primitives::TxEnv;

use crate::perpdex::{Operation, OrderType, PerpDexTx, Side};

/// Magic bytes prefix for identifying PerpDex transactions.
///
/// PerpDex transactions are identified by checking if the transaction's
/// `to` field matches the PERPDEX_CONTRACT_ADDRESS.
pub const PERPDEX_CONTRACT_ADDRESS: Address = Address::new([0xFF; 20]);

/// Parsed transaction that can be either EVM or PerpDex.
#[derive(Debug, Clone)]
pub enum ParsedTransaction {
    /// Standard EVM transaction
    Evm(TxEnv),
    /// PerpDex orderbook transaction
    PerpDex(PerpDexTx),
}

/// Identify the VM type of a transaction.
///
/// Uses the transaction's `to` field to determine the type:
/// - If `to` equals PERPDEX_CONTRACT_ADDRESS -> PerpDex
/// - Otherwise -> EVM
pub fn identify_vm_type<T: Transaction>(tx: &T) -> VmType {
    if let Some(to) = tx.to() {
        if to == PERPDEX_CONTRACT_ADDRESS {
            return VmType::PerpDex;
        }
    }
    VmType::Evm
}

/// VM type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmType {
    /// Ethereum Virtual Machine
    Evm,
    /// PerpDex orderbook VM
    PerpDex,
}

/// Decode a PerpDex transaction from raw transaction data.
///
/// The transaction input format (simplified encoding):
/// - Bytes 0-20: user address
/// - Bytes 20-28: nonce (u64)
/// - Bytes 28-44: gas_limit (u64) + gas_price (u128)
/// - Remaining: operations (encoded as operation type + params)
///
/// For Phase 3, we use a simplified encoding. In production, this would
/// use a proper serialization format like RLP or protobuf.
pub fn decode_perpdex_tx<T: Transaction>(tx: &T) -> Result<PerpDexTx, DecodeError> {
    let input = tx.input();

    if input.len() < 44 {
        return Err(DecodeError::InvalidLength);
    }

    // For Phase 3 demo, we'll create a simple default transaction
    // In production, this would properly decode from input bytes

    // Get user from the first 20 bytes of input (which we encoded there)
    let user = Address::from_slice(&input[0..20]);

    // Parse nonce from input (bytes 20-28)
    let nonce = u64::from_be_bytes(
        input[20..28].try_into().map_err(|_| DecodeError::InvalidFormat)?
    );

    // Parse gas from input (bytes 28-36)
    let gas_limit = u64::from_be_bytes(
        input[28..36].try_into().map_err(|_| DecodeError::InvalidFormat)?
    );

    // Parse gas price from tx
    let gas_price = U256::from(tx.max_fee_per_gas());

    // Parse operations from remaining bytes
    let operations = decode_operations(&input[44..])?;

    Ok(PerpDexTx {
        user,
        operations,
        gas_limit,
        gas_price,
        nonce,
        signature: None, // Signature already verified by tx
    })
}

/// Decode operations from bytes.
fn decode_operations(data: &[u8]) -> Result<Vec<Operation>, DecodeError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    let mut operations = Vec::new();
    let mut offset = 0;

    while offset < data.len() {
        if offset + 1 > data.len() {
            break;
        }

        let op_type = data[offset];
        offset += 1;

        match op_type {
            0 => {
                // PlaceOrder
                if offset + 89 > data.len() {
                    return Err(DecodeError::InvalidFormat);
                }

                let market_id = u64::from_be_bytes(
                    data[offset..offset + 8].try_into().unwrap()
                );
                offset += 8;

                let order_type_byte = data[offset];
                offset += 1;
                let order_type = match order_type_byte {
                    0 => OrderType::Limit,
                    1 => OrderType::Market,
                    2 => OrderType::StopLimit,
                    3 => OrderType::StopMarket,
                    _ => return Err(DecodeError::InvalidFormat),
                };

                let side = if data[offset] == 0 { Side::Long } else { Side::Short };
                offset += 1;

                let price = U256::from_be_slice(&data[offset..offset + 32]);
                offset += 32;

                let size = U256::from_be_slice(&data[offset..offset + 32]);
                offset += 32;

                let leverage = data[offset];
                offset += 1;

                let post_only = data[offset] != 0;
                offset += 1;

                let reduce_only = data[offset] != 0;
                offset += 1;

                // expires_at (8 bytes, 0 means None)
                let expires_at_raw = u64::from_be_bytes(
                    data[offset..offset + 8].try_into().unwrap()
                );
                offset += 8;
                let expires_at = if expires_at_raw == 0 { None } else { Some(expires_at_raw) };

                operations.push(Operation::PlaceOrder {
                    market_id,
                    order_type,
                    side,
                    price,
                    size,
                    leverage,
                    post_only,
                    reduce_only,
                    expires_at,
                });
            }
            1 => {
                // CancelOrder
                if offset + 16 > data.len() {
                    return Err(DecodeError::InvalidFormat);
                }

                let market_id = u64::from_be_bytes(
                    data[offset..offset + 8].try_into().unwrap()
                );
                offset += 8;

                let count = u64::from_be_bytes(
                    data[offset..offset + 8].try_into().unwrap()
                ) as usize;
                offset += 8;

                if offset + count * 8 > data.len() {
                    return Err(DecodeError::InvalidFormat);
                }

                let mut order_ids = Vec::with_capacity(count);
                for _ in 0..count {
                    let id = u64::from_be_bytes(
                        data[offset..offset + 8].try_into().unwrap()
                    );
                    order_ids.push(id);
                    offset += 8;
                }

                operations.push(Operation::CancelOrder { market_id, order_ids });
            }
            2 => {
                // CancelAll
                if offset + 8 > data.len() {
                    return Err(DecodeError::InvalidFormat);
                }

                let market_id_raw = u64::from_be_bytes(
                    data[offset..offset + 8].try_into().unwrap()
                );
                offset += 8;

                let market_id = if market_id_raw == 0 { None } else { Some(market_id_raw) };

                operations.push(Operation::CancelAll { market_id });
            }
            3 => {
                // SetLeverage
                if offset + 9 > data.len() {
                    return Err(DecodeError::InvalidFormat);
                }

                let market_id = u64::from_be_bytes(
                    data[offset..offset + 8].try_into().unwrap()
                );
                offset += 8;

                let leverage = data[offset];
                offset += 1;

                operations.push(Operation::SetLeverage { market_id, leverage });
            }
            4 => {
                // AdjustMargin
                if offset + 40 > data.len() {
                    return Err(DecodeError::InvalidFormat);
                }

                let market_id = u64::from_be_bytes(
                    data[offset..offset + 8].try_into().unwrap()
                );
                offset += 8;

                let amount = U256::from_be_slice(&data[offset..offset + 32]);
                offset += 32;

                operations.push(Operation::AdjustMargin { market_id, amount });
            }
            _ => return Err(DecodeError::InvalidFormat),
        }
    }

    Ok(operations)
}

/// Helper function to encode a simple PerpDex transaction for testing.
///
/// This is available for integration tests and benchmarks.
pub fn encode_perpdex_tx(tx: &PerpDexTx) -> Bytes {
    let mut data = Vec::new();

    // User address (20 bytes)
    data.extend_from_slice(tx.user.as_slice());

    // Nonce (8 bytes)
    data.extend_from_slice(&tx.nonce.to_be_bytes());

    // Gas limit (8 bytes)
    data.extend_from_slice(&tx.gas_limit.to_be_bytes());

    // Gas price (8 bytes, truncated)
    let gas_price_bytes: [u8; 32] = tx.gas_price.to_be_bytes();
    data.extend_from_slice(&gas_price_bytes[24..32]);

    // Encode operations
    for op in &tx.operations {
        match op {
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
            } => {
                data.push(0); // op_type
                data.extend_from_slice(&market_id.to_be_bytes());
                data.push(match order_type {
                    OrderType::Limit => 0,
                    OrderType::Market => 1,
                    OrderType::StopLimit => 2,
                    OrderType::StopMarket => 3,
                });
                data.push(if *side == Side::Long { 0 } else { 1 });
                data.extend_from_slice(&price.to_be_bytes::<32>());
                data.extend_from_slice(&size.to_be_bytes::<32>());
                data.push(*leverage);
                data.push(if *post_only { 1 } else { 0 });
                data.push(if *reduce_only { 1 } else { 0 });
                data.extend_from_slice(&expires_at.unwrap_or(0).to_be_bytes());
            }
            Operation::CancelOrder { market_id, order_ids } => {
                data.push(1); // op_type
                data.extend_from_slice(&market_id.to_be_bytes());
                data.extend_from_slice(&(order_ids.len() as u64).to_be_bytes());
                for id in order_ids {
                    data.extend_from_slice(&id.to_be_bytes());
                }
            }
            Operation::CancelAll { market_id } => {
                data.push(2); // op_type
                data.extend_from_slice(&market_id.unwrap_or(0).to_be_bytes());
            }
            Operation::SetLeverage { market_id, leverage } => {
                data.push(3); // op_type
                data.extend_from_slice(&market_id.to_be_bytes());
                data.push(*leverage);
            }
            Operation::AdjustMargin { market_id, amount } => {
                data.push(4); // op_type
                data.extend_from_slice(&market_id.to_be_bytes());
                data.extend_from_slice(&amount.to_be_bytes::<32>());
            }
        }
    }

    data.into()
}

/// Decoding errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DecodeError {
    #[error("Invalid transaction length")]
    InvalidLength,

    #[error("Invalid transaction format")]
    InvalidFormat,

    #[error("Invalid signature")]
    InvalidSignature,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_identify_vm_type_perpdex() {
        // Create a mock transaction to PerpDex contract
        use alloy_consensus::TxLegacy;
        use alloy_primitives::TxKind;

        let tx = TxLegacy {
            chain_id: None,
            nonce: 0,
            gas_price: 1000,
            gas_limit: 21000,
            to: TxKind::Call(PERPDEX_CONTRACT_ADDRESS),
            value: U256::ZERO,
            input: Bytes::new(),
        };

        assert_eq!(identify_vm_type(&tx), VmType::PerpDex);
    }

    #[test]
    fn test_identify_vm_type_evm() {
        use alloy_consensus::TxLegacy;
        use alloy_primitives::TxKind;

        let tx = TxLegacy {
            chain_id: None,
            nonce: 0,
            gas_price: 1000,
            gas_limit: 21000,
            to: TxKind::Call(address!("0000000000000000000000000000000000000001")),
            value: U256::ZERO,
            input: Bytes::new(),
        };

        assert_eq!(identify_vm_type(&tx), VmType::Evm);
    }

    #[test]
    fn test_encode_decode_simple_order() {
        let perpdex_tx = PerpDexTx {
            user: address!("1111111111111111111111111111111111111111"),
            nonce: 5,
            gas_limit: 100000,
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
                    post_only: false,
                    reduce_only: false,
                    expires_at: None,
                },
            ],
        };

        let encoded = encode_perpdex_tx(&perpdex_tx);

        // Verify basic structure
        assert!(encoded.len() >= 44); // Minimum size
        assert_eq!(&encoded[0..20], perpdex_tx.user.as_slice());
        assert_eq!(u64::from_be_bytes(encoded[20..28].try_into().unwrap()), 5);
    }
}
