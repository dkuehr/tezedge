// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::sync::{Arc, RwLock};

use failure::Fail;

use crate::merkle_storage::{MerkleStorage, MerkleError, ContextKey, ContextValue};
use crypto::hash::{BlockHash, ContextHash, HashType};
use crate::{BlockStorage, StorageError};
use crate::block_storage::BlockStorageReader;

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
    fn get_key_from_history(&self, context_hash: &ContextHash, key: &Vec<String>) -> Result<Option<Vec<u8>>, ContextError>;
    fn get_key_values_by_prefix(&self, context_hash: &ContextHash, prefix: &ContextKey) -> Result<Option<Vec<(ContextKey, ContextValue)>>, MerkleError>;
    fn level_to_hash(&self, level: i32) -> Result<ContextHash, ContextError>;
}

impl ContextApi for TezedgeContext {
    fn set(&mut self, context_hash: &Option<ContextHash>, key: Vec<String>, value: Vec<u8>) -> Result<(), ContextError> {
        //TODO ensure_eq_context_hash
        let mut merkle = self.merkle.write().expect("lock poisoning");
        merkle.checkout(context_hash.clone().unwrap());
        merkle.set(key, value);
        Ok(())
    }
    fn checkout(&self, context_hash: &ContextHash) -> Result<(), ContextError> {
        let mut merkle = self.merkle.write().expect("lock poisoning");
        merkle.checkout(context_hash.clone());
        Ok(())
    }
    fn commit(&mut self, block_hash: &BlockHash, parent_context_hash: &Option<ContextHash>,
              new_context_hash: &ContextHash, author: String, message: String,
              date: i64) -> Result<(), ContextError> {
        //TODO ensure_eq_context_hash
        //date == time?

        let mut merkle = self.merkle.write().expect("lock poisoning");
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
        let mut merkle = self.merkle.write().expect("lock poisoning");
        merkle.delete(key_prefix_to_delete.to_vec());
        Ok(())
    }
    fn remove_recursively_to_diff(&self, context_hash: &Option<ContextHash>, key_prefix_to_remove: &Vec<String>) -> Result<(), ContextError> {
        //TODO ensure_eq_context_hash
        let mut merkle = self.merkle.write().expect("lock poisoning");
        merkle.delete(key_prefix_to_remove.to_vec());
        Ok(())
    }
    fn copy_to_diff(&self, context_hash: &Option<ContextHash>, from_key: &Vec<String>, to_key: &Vec<String>) -> Result<(), ContextError> {
        //TODO ensure_eq_context_hash
        let mut merkle = self.merkle.write().expect("lock poisoning");
        merkle.copy(from_key.to_vec(), to_key.to_vec());
        Ok(())
    }
    fn get_key(&self, key: &Vec<String>) -> Result<Vec<u8>, ContextError> {
        let mut merkle = self.merkle.write().expect("lock poisoning");
        //TODO map error
        Ok(merkle.get(key).unwrap())
    }
    fn get_key_from_history(&self, context_hash: &ContextHash, key: &Vec<String>) -> Result<Option<Vec<u8>>, ContextError> {
        let mut merkle = self.merkle.write().expect("lock poisoning");
        match merkle.get_history(context_hash, key) {
            Err(MerkleError::ValueNotFound{key: _}) => Ok(None),
            Err(MerkleError::EntryNotFound) =>  {
                Err(ContextError::UnknownContextHashError { context_hash: hex::encode(context_hash).to_string() })
            },
            Err(err) => {
                Err(ContextError::ContextReadError { error: err })
            },
            Ok(val) => Ok(Some(val))
        }
    }
    fn get_key_values_by_prefix(&self, context_hash: &ContextHash, prefix: &ContextKey) -> Result<Option<Vec<(ContextKey, ContextValue)>>, MerkleError> {
        let mut merkle = self.merkle.write().expect("lock poisoning");
        // clients may pass in a prefix with elements containing slashes (expecting us to split)
        // we need to join with '/' and split again
        // TODO IMPORTANT: check if it's necessary to do the same thing in all other context methods
        let prefix = to_key(prefix).split('/').map(|s| s.to_string()).collect();
        merkle.get_key_values_by_prefix(context_hash, &prefix)
    }

    fn level_to_hash(&self, level: i32) -> Result<ContextHash, ContextError> {
        match self.block_storage.get_by_block_level(level) {
            Ok(Some(hash)) => {
                Ok(hash.header.context().to_vec())
            },
            _ => Err(ContextError::UnknownLevelError{level: level.to_string()})
        }
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
        error: MerkleError
    },
    #[fail(display = "Failed to read from context error: {}", error)]
    ContextReadError {
        error: MerkleError
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
        error: MerkleError,
    },
    #[fail(display = "Unknown level: {}", level)]
    UnknownLevelError {
        level: String,
    },
}
