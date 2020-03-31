use crate::pool::{AccountSeqNumberClient, UnverifiedUserTransaction};
use anyhow::Result;
use parking_lot::RwLock;
use starcoin_config::VMConfig;
use starcoin_executor::{executor::Executor, TransactionExecutor};
use starcoin_statedb::ChainStateDB;
use std::{collections::HashMap, fmt::Debug, sync::Arc};
use storage::StarcoinStorage;
use traits::ChainStateReader;
use types::{
    access_path::AccessPath,
    account_address::AccountAddress,
    account_config::AccountResource,
    block::BlockHeader,
    transaction,
    transaction::{CallError, SignedUserTransaction, TransactionError},
};

/// Cache for state nonces.
#[derive(Clone)]
pub struct NonceCache {
    nonces: Arc<RwLock<HashMap<AccountAddress, u64>>>,
    limit: usize,
}

impl NonceCache {
    /// Create new cache with a limit of `limit` entries.
    pub fn new(limit: usize) -> Self {
        NonceCache {
            nonces: Arc::new(RwLock::new(HashMap::with_capacity(limit / 2))),
            limit,
        }
    }

    /// Retrieve a cached nonce for given sender.
    pub fn get(&self, sender: &AccountAddress) -> Option<u64> {
        self.nonces.read().get(sender).cloned()
    }

    /// Clear all entries from the cache.
    pub fn clear(&self) {
        self.nonces.write().clear();
    }
}

impl std::fmt::Debug for NonceCache {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.debug_struct("NonceCache")
            .field("cache", &self.nonces.read().len())
            .field("limit", &self.limit)
            .finish()
    }
}

#[derive(Clone)]
pub struct CachedSeqNumberClient {
    statedb: Arc<ChainStateDB>,
    cache: NonceCache,
}

impl Debug for CachedSeqNumberClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedSequenceNumberClient")
            .field("cache", &self.cache.nonces.read().len())
            .field("limit", &self.cache.limit)
            .finish()
    }
}

impl CachedSeqNumberClient {
    pub fn new(statedb: ChainStateDB, cache: NonceCache) -> Self {
        Self {
            statedb: Arc::new(statedb),
            cache,
        }
    }

    fn latest_sequence_number(&self, address: &AccountAddress) -> u64 {
        let access_path = AccessPath::new_for_account(address.clone());
        let state = self
            .statedb
            .get(&access_path)
            .expect("read account state should ok");
        match state {
            None => 0u64,
            Some(s) => AccountResource::make_from(&s)
                .expect("account resource decode ok")
                .sequence_number(),
        }
    }
}

impl AccountSeqNumberClient for CachedSeqNumberClient {
    fn account_seq_number(&self, address: &AccountAddress) -> u64 {
        if let Some(nonce) = self.cache.get(address) {
            return nonce;
        }
        let mut cache = self.cache.nonces.write();
        let sequence_number = self.latest_sequence_number(address);
        cache.insert(*address, sequence_number);
        if cache.len() < self.cache.limit {
            return sequence_number;
        }

        debug!(target: "txpool", "NonceCache: reached limit");
        trace_time!("nonce_cache: clear");
        let to_remove: Vec<_> = cache.keys().take(self.cache.limit / 2).cloned().collect();
        for x in to_remove {
            cache.remove(&x);
        }

        sequence_number
    }
}

#[derive(Clone)]
pub struct PoolClient {
    best_block_header: BlockHeader,
    nonce_client: CachedSeqNumberClient,
}

impl std::fmt::Debug for PoolClient {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "PoolClient")
    }
}

impl PoolClient {
    pub fn new(
        best_block_header: BlockHeader,
        storage: Arc<StarcoinStorage>,
        cache: NonceCache,
    ) -> Self {
        let root_hash = best_block_header.state_root();
        let statedb = ChainStateDB::new(storage, Some(root_hash));
        let nonce_client = CachedSeqNumberClient::new(statedb, cache);
        Self {
            best_block_header,
            nonce_client,
        }
    }
}

impl crate::pool::AccountSeqNumberClient for PoolClient {
    fn account_seq_number(&self, address: &AccountAddress) -> u64 {
        self.nonce_client.account_seq_number(address)
    }
}

impl crate::pool::Client for PoolClient {
    fn verify_transaction(
        &self,
        tx: UnverifiedUserTransaction,
    ) -> Result<transaction::SignatureCheckedTransaction, transaction::TransactionError> {
        let txn = SignedUserTransaction::from(tx);
        let checked_txn = txn
            .clone()
            .check_signature()
            .map_err(|e| TransactionError::InvalidSignature(e.description().to_string()))?;
        let vmconfig = VMConfig::default();
        match Executor::validate_transaction(&vmconfig, self.nonce_client.statedb.as_ref(), txn) {
            None => Ok(checked_txn),
            Some(status) => {
                // Ok(checked_txn)
                Err(TransactionError::CallErr(CallError::Execution(status)))
            }
        }
    }
}