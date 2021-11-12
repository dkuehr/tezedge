// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::sync::Arc;
use redux_rs::Store;

use tezos_messages::p2p::{
    binary_message::MessageHash,
    encoding::{
        peer::{PeerMessageResponse, PeerMessage},
        current_head::CurrentHeadMessage,
        mempool::Mempool,
        operation::{GetOperationsMessage, OperationMessage},
    },
};

use tezos_api::ffi::BeginConstructionRequest;

use crate::{Action, ActionWithMeta, Service, State, service::{ProtocolService, RpcService}};
use crate::peer::message::{
    write::PeerMessageWriteInitAction,
    read::PeerMessageReadSuccessAction,
};

use super::{
    mempool_actions::{
        MempoolRecvDoneAction, MempoolGetOperationsAction, MempoolGetOperationsPendingAction,
        MempoolOperationRecvDoneAction, MempoolBroadcastAction, MempoolBroadcastDoneAction,
        MempoolOperationInjectAction, BlockAppliedAction,
    },
    mempool_state::HeadState,
};

pub fn mempool_effects<S>(
    store: &mut Store<State, S, Action>,
    action: &ActionWithMeta,
) where
    S: Service,
{
    match &action.action {
        Action::Protocol(_) => {
            // panic!("{:?}", act);
        },
        Action::PeerMessageReadSuccess(PeerMessageReadSuccessAction { message, address }) => {
            match message.message() {
                PeerMessage::CurrentHead(ref current_head) => {
                    let message = current_head.current_mempool().clone();
                    let head_state = HeadState {
                        chain_id: current_head.chain_id().clone(),
                        current_block: current_head.current_block_header().clone(),
                        // TODO(vlad): unwrap
                        current_block_hash: current_head.current_block_header().message_typed_hash().unwrap(),
                    };
                    store.dispatch(
                        MempoolRecvDoneAction {
                            address: *address,
                            head_state,
                            message,
                        },
                    );
                },
                PeerMessage::Operation(ref op) => {
                    store.dispatch(
                        MempoolOperationRecvDoneAction {
                            address: *address,
                            operation: op.clone().into(),
                        },
                    );
                },
                PeerMessage::GetOperations(ref hashes) => {
                    for hash in hashes.get_operations() {
                        let mempool = &store.state().mempool;
                        let op = None
                            .or_else(|| mempool.applied_operations.get(hash))
                            .or_else(|| mempool.branch_delayed_operations.get(hash))
                            .or_else(|| mempool.branch_refused_operations.get(hash))
                            .or_else(|| mempool.pending_operations.get(hash));

                        if let Some(op) = op {
                            let message = OperationMessage::from(op.clone());
                            store.dispatch(
                                PeerMessageWriteInitAction {
                                    address: *address,
                                    message: message.into(),
                                },
                            );
                        }
                    }
                },
                _ => (),
            }
        },
        Action::BlockApplied(BlockAppliedAction { chain_id, block }) => {
            // TODO: remove it
            let req = BeginConstructionRequest {
                chain_id: chain_id.clone(),
                predecessor: block.clone(),
                protocol_data: None,
            };
            store.service().protocol().begin_construction_for_mempool(req);
        },
        Action::MempoolRecvDone(MempoolRecvDoneAction { address, head_state, .. }) => {
            let mempool = &store.state().mempool;
            if matches!((&mempool.prevalidator_block, &mempool.local_head_state), (Some(b), Some(state)) if state.current_block_hash.ne(b)) {
                let req = BeginConstructionRequest {
                    chain_id: head_state.chain_id.clone(),
                    predecessor: head_state.current_block.clone(),
                    protocol_data: None,
                };
                store.service().protocol().begin_construction_for_mempool(req);
            }
            if let Some(peer) = store.state().mempool.peer_state.get(address) {
                if !peer.requesting_full_content.is_empty() {
                    store.dispatch(
                        MempoolGetOperationsAction {
                            address: *address,
                        },
                    );
                } else {
                    // if this mempool doesn't introduce new operations, we have nothing to do
                }
            }
        },
        Action::MempoolGetOperations(MempoolGetOperationsAction { address }) => {
            if let Some(peer) = store.state().mempool.peer_state.get(address) {
                let ops = peer.requesting_full_content.iter().cloned().collect();
                store.dispatch(
                    MempoolGetOperationsPendingAction {
                        address: *address,
                    },
                );
                store.dispatch(
                    PeerMessageWriteInitAction {
                        address: *address,
                        message: Arc::new(GetOperationsMessage::new(ops).into()),
                    },
                );
            }
        },
        Action::MempoolOperationRecvDone(MempoolOperationRecvDoneAction { address, .. }) => {
            let mempool_state = &store.state().mempool;
            if let Some(peer) = mempool_state.peer_state.get(address) {
                // received all pending operations from the particular peer
                if peer.pending_full_content.is_empty() {
                    if let Some(head_state) = peer.head_state.clone() {
                        let pending = mempool_state.pending_operations.keys();
                        let known_valid = mempool_state.applied_operations.keys()
                            .chain(mempool_state.branch_delayed_operations.keys())
                            .chain(mempool_state.branch_refused_operations.keys());
                        let known_valid = known_valid.cloned().collect();
                        let pending = pending.cloned().collect();
                        store.dispatch(
                            MempoolBroadcastAction {
                                address_exceptions: vec![*address],
                                head_state,
                                known_valid,
                                pending,
                            },
                        );
                    } else {
                        // should always have current head while waiting MempoolOperationRecvDone
                        // TODO(vlad): should be forbidden by enabling condition
                    }
                }
            }
        },
        Action::MempoolOperationInject(MempoolOperationInjectAction { rpc_id, .. }) => {
            let mempool_state = &store.state().mempool;
            // TODO(vlad): duplicated code
            if let Some(head_state) = mempool_state.local_head_state.clone() {
                let pending = mempool_state.pending_operations.keys().cloned().collect();
                let known_valid = mempool_state.applied_operations.keys()
                    .chain(mempool_state.branch_delayed_operations.keys())
                    .chain(mempool_state.branch_refused_operations.keys())
                    .cloned()
                    .collect();
                store.dispatch(
                    MempoolBroadcastAction {
                        address_exceptions: vec![],
                        head_state,
                        known_valid,
                        pending,
                    },
                );
                store.service().rpc().respond(*rpc_id, serde_json::Value::Null);
            } else {
                let resp = serde_json::Value::String("head is not ready".to_string());
                store.service().rpc().respond(*rpc_id, resp);
                // should always have current head while waiting MempoolOperationRecvDone
                // TODO(vlad): should be forbidden by enabling condition
            }
        },
        Action::MempoolBroadcast(MempoolBroadcastAction { address_exceptions, head_state, known_valid, pending }) => {
            let addresses = store.state().peers.iter_addr().cloned().collect::<Vec<_>>();
            // TODO(vlad): add action removing peer_state for disconnected peers
            for address in addresses {
                if address_exceptions.contains(&address) {
                    continue;
                }
                let peer = match store.state().mempool.peer_state.get(&address) {
                    Some(v) => v,
                    None => continue,
                };
                let known_valid = known_valid
                    .iter()
                    .filter(|hash| !peer.known_operations.contains(*hash))
                    .cloned()
                    .collect::<Vec<_>>();
                let pending = pending
                    .iter()
                    .filter(|hash| !peer.known_operations.contains(*hash))
                    .cloned()
                    .collect::<Vec<_>>();
                let message = CurrentHeadMessage::new(
                    head_state.chain_id.clone(),
                    head_state.current_block.clone(),
                    Mempool::new(known_valid.clone(), pending.clone()),
                );
                let message = Arc::new(PeerMessageResponse::from(message));

                store.dispatch(
                    PeerMessageWriteInitAction {
                        address,
                        message: message.clone(),
                    },
                );
                store.dispatch(
                    MempoolBroadcastDoneAction {
                        address,
                        pending,
                        known_valid,
                    },
                );
            }
        },
        _ => (),
    }
}
