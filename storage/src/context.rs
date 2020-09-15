// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::sync::{Arc, RwLock};

use failure::Fail;

use crate::merkle_storage::MerkleStorage;
use crypto::hash::{BlockHash, ContextHash, HashType};
use crate::{BlockStorage, StorageError};

/// Abstraction on context manipulation
pub trait ContextApi {
    fn set(&mut self, context_hash: &Option<ContextHash>, key: Vec<String>, value: Vec<u8>) -> Result<(), ContextError>;
    fn checkout(&self, context_hash: &ContextHash) -> Result<(), ContextError>; 
    fn commit(&mut self, block_hash: &BlockHash, parent_context_hash: &Option<ContextHash>,
              new_context_hash: &ContextHash, author: String, message: String,
              date: i64) -> Result<(), ContextError>;
    
    fn delete_to_diff(&self, context_hash: &Option<ContextHash>, key_prefix_to_delete: &Vec<String>) -> Result<(), ContextError>;
    fn remove_recursively_to_diff(&self, context_hash: &Option<ContextHash>, key_prefix_to_remove: &Vec<String>) -> Result<(), ContextError>;
    fn copy_to_diff(&self, context_hash: &Option<ContextHash>, from_key: &Vec<String>, to_key: &Vec<String>) -> Result<(), ContextError>;
    fn get_key(&self, key: &Vec<String>) -> Result<Vec<u8>, ContextError>;
}

impl ContextApi for TezedgeContext {
    fn set(&mut self, context_hash: &Option<ContextHash>, key: Vec<String>, value: Vec<u8>) -> Result<(), ContextError> {
        //TODO ensure_eq_context_hash
        let mut merkle = self.merkle.write().unwrap();
        merkle.checkout(context_hash.clone().unwrap());
        merkle.set(key, value);
        Ok(())
    }
    fn checkout(&self, context_hash: &ContextHash) -> Result<(), ContextError> {
        let mut merkle = self.merkle.write().unwrap();
        merkle.checkout(context_hash.clone());
        Ok(())
    }
    fn commit(&mut self, block_hash: &BlockHash, parent_context_hash: &Option<ContextHash>,
              new_context_hash: &ContextHash, author: String, message: String,
              date: i64) -> Result<(), ContextError> {
        //TODO ensure_eq_context_hash
        //date == time?

        let mut merkle = self.merkle.write().unwrap();
        merkle.commit(date as u64, author, message);

        // associate block and context_hash
        if let Err(e) = self.block_storage.assign_to_context(block_hash, new_context_hash) {
            match e {
                StorageError::MissingKey => {
                    if parent_context_hash.is_some() {
                        return Err(
                            ContextError::ContextHashAssignError {
                                block_hash: HashType::BlockHash.bytes_to_string(block_hash),
                                context_hash: HashType::ContextHash.bytes_to_string(new_context_hash),
                                error: e,
                            }
                        );
                    } else {
                        // if parent_context_hash is empty, means it is commit_genesis, and block is not already stored, thats ok
                        ()
                    }
                }
                _ => return Err(
                    ContextError::ContextHashAssignError {
                        block_hash: HashType::BlockHash.bytes_to_string(block_hash),
                        context_hash: HashType::ContextHash.bytes_to_string(new_context_hash),
                        error: e,
                    }
                )
            };
        }

        Ok(())
    }
    
    fn delete_to_diff(&self, context_hash: &Option<ContextHash>, key_prefix_to_delete: &Vec<String>) -> Result<(), ContextError> {
        //TODO ensure_eq_context_hash
        let mut merkle = self.merkle.write().unwrap();
        merkle.delete(key_prefix_to_delete.to_vec());
        Ok(())
    }
    fn remove_recursively_to_diff(&self, context_hash: &Option<ContextHash>, key_prefix_to_remove: &Vec<String>) -> Result<(), ContextError> {
        //TODO ensure_eq_context_hash
        let mut merkle = self.merkle.write().unwrap();
        merkle.delete(key_prefix_to_remove.to_vec());
        Ok(())
    }
    fn copy_to_diff(&self, context_hash: &Option<ContextHash>, from_key: &Vec<String>, to_key: &Vec<String>) -> Result<(), ContextError> {
        //TODO ensure_eq_context_hash
        let mut merkle = self.merkle.write().unwrap();
        merkle.copy(from_key.to_vec(), to_key.to_vec());
        Ok(())
    }
    fn get_key(&self, key: &Vec<String>) -> Result<Vec<u8>, ContextError> {
        let mut merkle = self.merkle.write().unwrap();
        Ok(merkle.get(key).unwrap())
    }
}

fn to_key(key: &Vec<String>) -> String {
    key.join("/")
}

pub struct TezedgeContext {
    block_storage: BlockStorage,
//    storage: ContextList,
    merkle: Arc<RwLock<MerkleStorage>>,
}

impl TezedgeContext {
    pub fn new(block_storage: BlockStorage, merkle: Arc<RwLock<MerkleStorage>>) -> Self {
        TezedgeContext { block_storage, merkle }
    }
}

/// Possible errors for context
#[derive(Debug, Fail)]
pub enum ContextError {
    #[fail(display = "Failed to save commit error: {}", error)]
    CommitWriteError {
        error: StorageError
    },
    #[fail(display = "Failed to read from context error: {}", error)]
    ContextReadError {
        error: StorageError
    },
    #[fail(display = "Failed to assign context_hash: {:?} to block_hash: {}, error: {}", context_hash, block_hash, error)]
    ContextHashAssignError {
        context_hash: String,
        block_hash: String,
        error: StorageError,
    },
    #[fail(display = "InvalidContextHash for context diff to commit, expected_context_hash: {:?}, context_hash: {:?}", expected_context_hash, context_hash)]
    InvalidContextHashError {
        expected_context_hash: Option<String>,
        context_hash: Option<String>,
    },
    #[fail(display = "Unknown context_hash: {:?}", context_hash)]
    UnknownContextHashError {
        context_hash: String,
    },
    #[fail(display = "Failed to read block for context_hash: {:?}, error: {}", context_hash, error)]
    ReadBlockError {
        context_hash: String,
        error: StorageError,
    },
}

