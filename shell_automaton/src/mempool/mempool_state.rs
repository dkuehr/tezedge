// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::{collections::{HashMap, HashSet}, net::SocketAddr};

use serde::{Serialize, Deserialize};

use crypto::hash::{OperationHash, ChainId, BlockHash};
use tezos_messages::p2p::{
    encoding::{block_header::BlockHeader, operation::Operation},
};
use tezos_api::ffi::{PrevalidatorWrapper, Applied, Errored};

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct MempoolState {
    // all blocks applied
    pub(super) applied_block: HashSet<BlockHash>,
    // do not create prevalidator for any applied block, create prevalidator:
    // * for block received as CurrentHead
    // * for block of injected operation
    pub(super) prevalidator: Option<PrevalidatorWrapper>,
    //
    pub(super) requesting_prevalidator_for: Option<BlockHash>,
    // the current head applied
    pub(super) local_head_state: Option<HeadState>,
    // let's track what our peers know, and what we waiting from them
    pub(super) peer_state: HashMap<SocketAddr, PeerState>,
    // operations that passed basic checks, but not protocol
    pub(super) pending_operations: HashMap<OperationHash, Operation>,
    pub validated_operations: ValidatedOperations,
}

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct ValidatedOperations {
    pub ops: HashMap<OperationHash, Operation>,
    pub refused_ops: HashMap<OperationHash, Operation>,
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
    pub current_block: BlockHeader,
}

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct PeerState {
    // the current head of the peer
    pub(super) head_state: Option<HeadState>,
    // we received mempool from the peer and gonna send GetOperations
    pub(super) requesting_full_content: HashSet<OperationHash>,
    // we sent GetOperations and pending full content of those operations
    pub(super) pending_full_content: HashSet<OperationHash>,
    // those operations are known to the peer, should not rebroadcast
    pub(super) known_operations: HashSet<OperationHash>,
}
