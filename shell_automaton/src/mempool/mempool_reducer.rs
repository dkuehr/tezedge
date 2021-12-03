// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::convert::TryInto;

use slog::{debug, error, warn};
use tezos_messages::p2p::binary_message::MessageHash;

use crate::mempool::mempool_state::OperationState;
use crate::prechecker::{
    PrecheckerPrecheckOperationResponse, PrecheckerPrecheckOperationResponseAction,
};
use crate::protocol::ProtocolAction;
use crate::{Action, ActionWithMeta, State};

use super::mempool_state::{MempoolCurrentHead, MempoolOperation};
use super::{
    BlockAppliedAction, HeadState, MempoolBroadcastDoneAction,
    MempoolCleanupWaitPrevalidatorAction, MempoolGetOperationsPendingAction,
    MempoolOperationDecodedAction, MempoolOperationInjectAction, MempoolOperationRecvDoneAction,
    MempoolRecvDoneAction, MempoolRpcRespondAction, MempoolValidateWaitPrevalidatorAction,
};

pub fn mempool_reducer(state: &mut State, action: &ActionWithMeta) {
    if state.config.disable_mempool {
        return;
    }
    let mut mempool_state = &mut state.mempool;

    match &action.action {
        Action::Protocol(act) => match act {
            ProtocolAction::PrevalidatorForMempoolReady(prevalidator) => {
                mempool_state.prevalidator = Some(prevalidator.clone());
            }
            ProtocolAction::OperationValidated(result) => {
                mempool_state.prevalidator = Some(result.prevalidator.clone());
                for v in &result.result.applied {
                    if let Some(op) = mempool_state.pending_operations.remove(&v.hash) {
                        mempool_state
                            .validated_operations
                            .ops
                            .insert(v.hash.clone().into(), op);
                        mempool_state.validated_operations.applied.push(v.clone());
                    }
                    if let Some(rpc_id) = mempool_state.injecting_rpc_ids.remove(&v.hash) {
                        mempool_state
                            .injected_rpc_ids
                            .insert(v.hash.clone().into(), rpc_id);
                    }
                    if let Some(operation_state) = mempool_state.operations_state.get_mut(&v.hash) {
                        if let MempoolOperation {
                            state: OperationState::Decoded,
                            ..
                        } = operation_state
                        {
                            *operation_state =
                                operation_state.next_state(OperationState::Applied, action);
                        }
                    }
                }
                for v in &result.result.refused {
                    if let Some(op) = mempool_state.pending_operations.remove(&v.hash) {
                        mempool_state
                            .validated_operations
                            .refused_ops
                            .insert(v.hash.clone().into(), op);
                        mempool_state.validated_operations.refused.push(v.clone());
                    }
                    if let Some(rpc_id) = mempool_state.injecting_rpc_ids.remove(&v.hash) {
                        mempool_state
                            .injected_rpc_ids
                            .insert(v.hash.clone().into(), rpc_id);
                    }
                    if let Some(operation_state) = mempool_state.operations_state.get_mut(&v.hash) {
                        if let MempoolOperation {
                            state: OperationState::Decoded,
                            ..
                        } = operation_state
                        {
                            *operation_state =
                                operation_state.next_state(OperationState::Refused, action);
                        }
                    }
                }
                for v in &result.result.branch_refused {
                    if let Some(op) = mempool_state.pending_operations.remove(&v.hash) {
                        mempool_state
                            .validated_operations
                            .ops
                            .insert(v.hash.clone().into(), op);
                        mempool_state
                            .validated_operations
                            .branch_refused
                            .push(v.clone());
                    }
                    if let Some(rpc_id) = mempool_state.injecting_rpc_ids.remove(&v.hash) {
                        mempool_state
                            .injected_rpc_ids
                            .insert(v.hash.clone().into(), rpc_id);
                    }
                    if let Some(operation_state) = mempool_state.operations_state.get_mut(&v.hash) {
                        if let MempoolOperation {
                            state: OperationState::Decoded,
                            ..
                        } = operation_state
                        {
                            *operation_state =
                                operation_state.next_state(OperationState::BranchRefused, action);
                        }
                    }
                }
                for v in &result.result.branch_delayed {
                    if let Some(op) = mempool_state.pending_operations.remove(&v.hash) {
                        mempool_state
                            .validated_operations
                            .ops
                            .insert(v.hash.clone().into(), op);
                        mempool_state
                            .validated_operations
                            .branch_delayed
                            .push(v.clone());
                    }
                    if let Some(rpc_id) = mempool_state.injecting_rpc_ids.remove(&v.hash) {
                        mempool_state
                            .injected_rpc_ids
                            .insert(v.hash.clone().into(), rpc_id);
                    }
                    if let Some(operation_state) = mempool_state.operations_state.get_mut(&v.hash) {
                        if let MempoolOperation {
                            state: OperationState::Decoded,
                            ..
                        } = operation_state
                        {
                            *operation_state =
                                operation_state.next_state(OperationState::BranchDelayed, action);
                        }
                    }
                }
            }
            act => {
                println!("{:?}", act);
            }
        },
        Action::BlockApplied(BlockAppliedAction {
            chain_id, block, ..
        }) => {
            let head_state = match HeadState::new(chain_id.clone(), block.clone()) {
                Ok(v) => v,
                Err(err) => {
                    error!(&state.log, "Cannot calculate block header hash"; "error" => err.to_string());
                    return;
                }
            };
            mempool_state
                .applied_block
                .insert(head_state.block_hash.clone());
            if let Some(current_head) = mempool_state.current_heads.get(&head_state.block_hash) {
                let time = action.id.duration_since(current_head.stamp);
                debug!(&state.log, "======== new block applied"; "hash" => head_state.block_hash.to_string(), "time" => format!("{:?}", time));
            }
            mempool_state.local_head_state = Some(head_state);
        }
        Action::MempoolRecvDone(MempoolRecvDoneAction {
            address,
            message,
            head_state,
        }) => {
            let log = state.log.clone();
            mempool_state
                .current_heads
                .entry(head_state.block_hash.clone().into())
                .or_insert_with(|| {
                    debug!(log, "======== new block received"; "hash" => head_state.block_hash.to_string());
                    MempoolCurrentHead::new(head_state, action.id)})
                .peers
                .insert(*address);

            match &mempool_state.latest_current_head {
                Some(latest) if latest == &head_state.block_hash => (),
                Some(latest) => {
                    if let Some(latest_head) = mempool_state.current_heads.get(latest) {
                        if latest_head.level == head_state.current_block.level() {
                            warn!(state.log, "======== different block on the same level"; "level" => latest_head.level);
                        } else {
                            if latest_head.level < head_state.current_block.level() {
                                if latest_head.level + 1 != head_state.current_block.level() {
                                    warn!(state.log, "======== jumping over blocks"; "old_level" => latest_head.level, "new_level" => head_state.current_block.level());
                                }
                                mempool_state.latest_current_head =
                                    Some(head_state.block_hash.clone());
                            }
                        }
                    }
                }
                _ => {
                    mempool_state.latest_current_head = Some(head_state.block_hash.clone());
                }
            }

            let pending = message.pending().iter().cloned();
            let known_valid = message.known_valid().iter().cloned();

            let block_time = head_state
                .current_block
                .timestamp()
                .try_into()
                .unwrap_or(action.id.into());

            let peer = mempool_state.peer_state.entry(*address).or_default();
            for hash in pending.chain(known_valid) {
                let known = mempool_state.pending_operations.contains_key(&hash)
                    || mempool_state.validated_operations.ops.contains_key(&hash);
                if !known {
                    peer.requesting_full_content.insert(hash.clone());
                    // of course peer knows about it, because he sent us it
                    peer.seen_operations.insert(hash.clone());

                    mempool_state.operations_state.insert(
                        hash.into(),
                        MempoolOperation::received(&head_state.block_hash, block_time, action),
                    );
                }
            }
        }
        Action::MempoolGetOperationsPending(MempoolGetOperationsPendingAction { address }) => {
            let peer = mempool_state.peer_state.entry(*address).or_default();
            peer.pending_full_content
                .extend(peer.requesting_full_content.drain());
        }
        Action::MempoolOperationRecvDone(MempoolOperationRecvDoneAction { address, operation }) => {
            let operation_hash = match operation.message_typed_hash() {
                Ok(v) => v,
                Err(err) => {
                    // TODO(vlad): peer send bad operation, should log the error,
                    // maybe should disconnect the peer
                    let _ = err;
                    return;
                }
            };
            let peer = mempool_state.peer_state.entry(*address).or_default();

            if !peer.pending_full_content.remove(&operation_hash) {
                // TODO(vlad): received operation, but we did not requested it, what should we do?
            }

            mempool_state
                .pending_operations
                .insert(operation_hash.into(), operation.clone());
        }
        Action::MempoolOperationInject(MempoolOperationInjectAction {
            operation,
            operation_hash,
            rpc_id,
        }) => {
            mempool_state
                .injecting_rpc_ids
                .insert(operation_hash.clone().into(), rpc_id.clone());
            mempool_state
                .pending_operations
                .insert(operation_hash.clone().into(), operation.clone());
        }
        Action::MempoolValidateWaitPrevalidator(MempoolValidateWaitPrevalidatorAction {
            operation,
        }) => {
            // TODO(vlad): hash
            mempool_state
                .wait_prevalidator_operations
                .push(operation.clone());
        }
        Action::MempoolCleanupWaitPrevalidator(MempoolCleanupWaitPrevalidatorAction {}) => {
            mempool_state.wait_prevalidator_operations.clear();
        }
        Action::PrecheckerPrecheckOperationResponse(
            PrecheckerPrecheckOperationResponseAction { response },
        ) => match response {
            PrecheckerPrecheckOperationResponse::Applied(applied) => {
                let hash = &applied.hash;
                if let Some(op) = mempool_state.pending_operations.remove(hash) {
                    mempool_state
                        .validated_operations
                        .ops
                        .insert(hash.clone().into(), op);
                    mempool_state
                        .validated_operations
                        .applied
                        .push(applied.as_applied());
                }
                if let Some(rpc_id) = mempool_state.injecting_rpc_ids.remove(hash) {
                    mempool_state
                        .injected_rpc_ids
                        .insert(hash.clone().into(), rpc_id);
                }
                if let Some(operation_state) = mempool_state.operations_state.get_mut(hash) {
                    if let MempoolOperation {
                        state: OperationState::Decoded,
                        ..
                    } = operation_state
                    {
                        *operation_state =
                            operation_state.next_state(OperationState::Prechecked, action);
                    }
                }
            }
            PrecheckerPrecheckOperationResponse::Refused(errored) => {
                let hash = &errored.hash;
                if let Some(op) = mempool_state.pending_operations.remove(&errored.hash) {
                    mempool_state
                        .validated_operations
                        .refused_ops
                        .insert(errored.hash.clone().into(), op);
                    mempool_state
                        .validated_operations
                        .refused
                        .push(errored.as_errored());
                }
                if let Some(rpc_id) = mempool_state.injecting_rpc_ids.remove(&errored.hash) {
                    mempool_state
                        .injected_rpc_ids
                        .insert(errored.hash.clone().into(), rpc_id);
                }
                if let Some(operation_state) = mempool_state.operations_state.get_mut(hash) {
                    if let MempoolOperation {
                        state: OperationState::Decoded,
                        ..
                    } = operation_state
                    {
                        let next =
                            operation_state.next_state(OperationState::PrecheckRefused, action);
                        *operation_state = next;
                    }
                }
            }
            PrecheckerPrecheckOperationResponse::Prevalidate(_) => {
                // TODO???
            }
            PrecheckerPrecheckOperationResponse::Error(_) => {
                // TODO
            }
        },
        Action::MempoolRpcRespond(MempoolRpcRespondAction {}) => {
            state.mempool.injected_rpc_ids.clear();
        }
        Action::MempoolBroadcastDone(MempoolBroadcastDoneAction {
            address,
            known_valid,
            pending,
        }) => {
            let peer = mempool_state.peer_state.entry(*address).or_default();

            peer.seen_operations.extend(known_valid.iter().cloned());
            peer.seen_operations.extend(pending.iter().cloned());

            for hash in known_valid {
                if let Some(operation_state) = mempool_state.operations_state.get_mut(hash) {
                    match operation_state {
                        MempoolOperation {
                            state: OperationState::Prechecked,
                            ..
                        }
                        | MempoolOperation {
                            state: OperationState::Applied,
                            ..
                        } => *operation_state = operation_state.broadcast(action),
                        _ => (),
                    }
                }
            }
        }

        Action::MempoolOperationDecoded(MempoolOperationDecodedAction {
            operation,
            protocol_data,
        }) => {
            if let Some(operation_state) = mempool_state.operations_state.get_mut(operation) {
                if let MempoolOperation {
                    state: OperationState::Received,
                    ..
                } = operation_state
                {
                    *operation_state = operation_state.decoded(protocol_data, action);
                }
            }
        }
        _ => (),
    }
}
