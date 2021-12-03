// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    net::SocketAddr,
};

use redux_rs::ActionId;
use serde::{Deserialize, Serialize};

use crypto::hash::{BlockHash, ChainId, HashBase58, OperationHash};
use tezos_api::ffi::{Applied, Errored, PrevalidatorWrapper};
use tezos_messages::p2p::{
    binary_message::{MessageHash, MessageHashError},
    encoding::{
        block_header::{BlockHeader, Level},
        operation::Operation,
    },
};

use crate::{service::rpc_service::RpcId, ActionWithMeta};

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct MempoolState {
    // all blocks applied
    pub(super) applied_block: HashSet<BlockHash>,
    // do not create prevalidator for any applied block, create prevalidator:
    // * for block received as CurrentHead
    // * for block of injected operation
    pub prevalidator: Option<PrevalidatorWrapper>,
    //
    pub(super) requesting_prevalidator_for: Option<BlockHash>,
    // performing rpc
    pub(super) injecting_rpc_ids: HashMap<HashBase58<OperationHash>, RpcId>,
    // performed rpc
    pub(super) injected_rpc_ids: HashMap<HashBase58<OperationHash>, RpcId>,
    // the current head applied
    pub local_head_state: Option<HeadState>,
    // let's track what our peers know, and what we waiting from them
    pub(super) peer_state: HashMap<SocketAddr, PeerState>,
    // operations that passed basic checks, sent to protocol validator
    pub(super) pending_operations: HashMap<HashBase58<OperationHash>, Operation>,
    // operations that passed basic checks, are not sent because prevalidator is not ready
    pub(super) wait_prevalidator_operations: Vec<Operation>,
    pub validated_operations: ValidatedOperations,

    pub operations_state: BTreeMap<HashBase58<OperationHash>, MempoolOperation>,

    pub current_heads: BTreeMap<HashBase58<BlockHash>, MempoolCurrentHead>,
    pub latest_current_head: Option<BlockHash>,
}

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct ValidatedOperations {
    pub ops: HashMap<HashBase58<OperationHash>, Operation>,
    pub refused_ops: HashMap<HashBase58<OperationHash>, Operation>,
    // operations that passed all checks and classified
    // can be applied in the current context
    pub applied: Vec<Applied>,
    // cannot be included in the next head of the chain, but it could be included in a descendant
    pub branch_delayed: Vec<Errored>,
    // might be applied on a different branch if a reorganization happens
    pub branch_refused: Vec<Errored>,
    pub refused: Vec<Errored>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HeadState {
    pub chain_id: ChainId,
    pub block_hash: BlockHash,
    pub current_block: BlockHeader,
}

impl HeadState {
    pub(super) fn new(
        chain_id: ChainId,
        current_block: BlockHeader,
    ) -> Result<Self, MessageHashError> {
        let block_hash = current_block.message_typed_hash::<BlockHash>()?;
        Ok(Self {
            chain_id,
            block_hash,
            current_block,
        })
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct PeerState {
    // we received mempool from the peer and gonna send GetOperations
    pub(super) requesting_full_content: HashSet<OperationHash>,
    // we sent GetOperations and pending full content of those operations
    pub(super) pending_full_content: HashSet<OperationHash>,
    // those operations are known to the peer, should not rebroadcast
    pub(super) seen_operations: HashSet<OperationHash>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MempoolOperation {
    pub branch: BlockHash,
    pub block_timestamp: u64,
    #[serde(flatten)]
    pub times: HashMap<String, u64>,
    //#[serde(flatten)]
    pub state: OperationState,
    pub protocol_data: Option<serde_json::Value>,
}

impl MempoolOperation {
    pub(super) fn received(
        branch: &BlockHash,
        mut block_timestamp: u64,
        action: &ActionWithMeta,
    ) -> Self {
        let state = OperationState::Received;
        block_timestamp *= 1_000_000_000;
        Self {
            branch: branch.clone(),
            block_timestamp,
            protocol_data: None,
            times: HashMap::from([(state.time_name(), action.time_as_nanos() - block_timestamp)]),
            state,
        }
    }

    pub(super) fn decoded(
        &self,
        protocol_data: &serde_json::Value,
        action: &ActionWithMeta,
    ) -> Self {
        let state = OperationState::Decoded;
        let mut times = self.times.clone();
        times.insert(
            state.time_name(),
            action.time_as_nanos() - self.block_timestamp,
        );
        Self {
            branch: self.branch.clone(),
            protocol_data: Some(protocol_data.clone()),
            block_timestamp: self.block_timestamp,
            times,
            state,
        }
    }

    pub(super) fn next_state(&self, state: OperationState, action: &ActionWithMeta) -> Self {
        let mut times = self.times.clone();
        times.insert(
            state.time_name(),
            action.time_as_nanos() - self.block_timestamp,
        );
        Self {
            branch: self.branch.clone(),
            protocol_data: self.protocol_data.clone(),
            block_timestamp: self.block_timestamp,
            times,
            state,
        }
    }

    pub(super) fn for_branch(&self, branch: &BlockHash) -> bool {
        &self.branch == branch
    }

    pub(super) fn endorsement_slot(&self) -> Option<&serde_json::Value> {
        let contents = self
            .protocol_data
            .as_ref()?
            .as_object()?
            .get("contents")?
            .as_array()?;
        let contents_0 = if contents.len() == 1 {
            contents.get(0)?.as_object()?
        } else {
            return None;
        };
        match contents_0.get("kind")?.as_str()? {
            "endorsement_with_slot" => contents_0.get("slot"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, strum_macros::Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OperationState {
    Received,
    Decoded,
    Prechecked,
    Applied,
    Broadcast,

    PrecheckRefused,
    Refused,
    BranchRefused,
    BranchDelayed,
}

impl OperationState {
    fn time_name(&self) -> String {
        self.to_string() + "_time"
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MempoolCurrentHead {
    pub chain_id: ChainId,
    pub level: Level,
    pub predecessor: BlockHash,
    pub peers: BTreeSet<SocketAddr>,
    pub stamp: ActionId,
}

impl MempoolCurrentHead {
    pub(super) fn new(head_state: &HeadState, stamp: ActionId) -> Self {
        Self {
            chain_id: head_state.chain_id.clone(),
            level: head_state.current_block.level(),
            predecessor: head_state.current_block.predecessor().clone(),
            peers: BTreeSet::new(),
            stamp,
        }
    }
}
