// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use crate::block_applier::{BlockApplierApplyState, BlockApplierEnqueueBlockAction};
use crate::mempool::mempool_actions::{
    BlockInjectAction, MempoolAskCurrentHeadAction, MempoolGetPendingOperationsAction,
    MempoolOperationInjectAction, MempoolRegisterOperationsStreamAction,
    MempoolRemoveAppliedOperationsAction, MempoolRpcEndorsementsStatusGetAction,
};
use crate::rights::{rights_actions::RightsRpcGetAction, RightsKey};
use crate::service::rpc_service::{RpcRequest, RpcRequestStream};
use crate::service::{RpcService, Service};
use crate::{Action, ActionWithMeta, Store};

use super::rpc_actions::{
    RpcBootstrappedAction, RpcBootstrappedDoneAction, RpcBootstrappedNewBlockAction,
};
use super::BootstrapState;

pub fn rpc_effects<S: Service>(store: &mut Store<S>, action: &ActionWithMeta) {
    match &action.action {
        Action::WakeupEvent(_) => {
            while let Ok((msg, rpc_id)) = store.service().rpc().try_recv_stream() {
                match msg {
                    RpcRequestStream::Bootstrapped => {
                        store.dispatch(RpcBootstrappedAction { rpc_id });
                    }
                    RpcRequestStream::GetOperations {
                        applied,
                        refused,
                        branch_delayed,
                        branch_refused,
                    } => {
                        store.dispatch(MempoolRegisterOperationsStreamAction {
                            rpc_id,
                            applied,
                            refused,
                            branch_delayed,
                            branch_refused,
                        });
                    }
                }
            }
            while let Ok((msg, rpc_id)) = store.service().rpc().try_recv() {
                match msg {
                    RpcRequest::GetCurrentGlobalState { channel } => {
                        let _ = channel.send(store.state.get().clone());
                        store
                            .service()
                            .rpc()
                            .respond(rpc_id, serde_json::Value::Null);
                    }
                    RpcRequest::GetMempoolOperationStats { channel } => {
                        let stats = store.state().mempool.operation_stats.clone();
                        let _ = channel.send(stats);
                        store
                            .service()
                            .rpc()
                            .respond(rpc_id, serde_json::Value::Null);
                    }
                    RpcRequest::GetBlockStats { channel } => {
                        let stats = store.service().statistics();
                        let _ = channel.send(stats.map(|s| s.block_stats_get_all().clone()));
                        store
                            .service()
                            .rpc()
                            .respond(rpc_id, serde_json::Value::Null);
                    }
                    RpcRequest::InjectBlock { block_hash } => {
                        store.dispatch(BlockApplierEnqueueBlockAction {
                            block_hash: block_hash.into(),
                            injector_rpc_id: Some(rpc_id),
                        });
                    }
                    RpcRequest::InjectOperation {
                        operation,
                        operation_hash,
                    } => {
                        store.dispatch(MempoolOperationInjectAction {
                            operation,
                            operation_hash,
                            rpc_id,
                        });
                    }
                    RpcRequest::InjectBlockStart {
                        chain_id,
                        block_hash,
                        block_header,
                    } => {
                        store.dispatch(BlockInjectAction {
                            chain_id,
                            block_hash,
                            block_header,
                        });
                    }
                    RpcRequest::RequestCurrentHeadFromConnectedPeers => {
                        store.dispatch(MempoolAskCurrentHeadAction {});
                        store
                            .service()
                            .rpc()
                            .respond(rpc_id, serde_json::Value::Null);
                    }
                    RpcRequest::RemoveOperations { operation_hashes } => {
                        store.dispatch(MempoolRemoveAppliedOperationsAction { operation_hashes });
                        store
                            .service()
                            .rpc()
                            .respond(rpc_id, serde_json::Value::Null);
                    }
                    RpcRequest::MempoolStatus => match &store.state().mempool.running_since {
                        None => store
                            .service()
                            .rpc()
                            .respond(rpc_id, serde_json::Value::Null),
                        Some(()) => store
                            .service()
                            .rpc()
                            .respond(rpc_id, serde_json::Value::String("".to_string())),
                    },
                    RpcRequest::GetPendingOperations => {
                        store.dispatch(MempoolGetPendingOperationsAction { rpc_id });
                    }
                    RpcRequest::GetBakingRights { block_hash, level } => {
                        store.dispatch(RightsRpcGetAction {
                            key: RightsKey::baking(block_hash, level, None),
                            rpc_id,
                        });
                    }
                    RpcRequest::GetEndorsingRights { block_hash, level } => {
                        store.dispatch(RightsRpcGetAction {
                            key: RightsKey::endorsing(block_hash, level),
                            rpc_id,
                        });
                    }
                    RpcRequest::GetEndorsementsStatus { block_hash } => {
                        store
                            .dispatch(MempoolRpcEndorsementsStatusGetAction { rpc_id, block_hash });
                    }

                    RpcRequest::GetStatsCurrentHeadStats { channel, level } => {
                        let _ = channel.send(
                            store
                                .service
                                .statistics()
                                .map_or(Vec::new(), |s| s.block_stats_get_by_level(level)),
                        );
                    }
                }
            }
        }

        Action::CurrentHeadUpdate(content) => {
            let is_bootstrapped = store.state().is_bootstrapped();
            store.dispatch(RpcBootstrappedNewBlockAction {
                block: content.new_head.hash.clone(),
                timestamp: content.new_head.header.timestamp(),
                is_bootstrapped,
            });
        }

        Action::RpcBootstrapped(RpcBootstrappedAction { rpc_id }) => {
            if let Some(BootstrapState {
                json,
                is_bootstrapped,
            }) = &store.state.get().rpc.bootstrapped.state
            {
                store
                    .service
                    .rpc()
                    .respond_stream(*rpc_id, Some(json.clone()));
                if *is_bootstrapped {
                    store.dispatch(RpcBootstrappedDoneAction {
                        rpc_ids: vec![*rpc_id],
                    });
                }
            }
        }
        Action::RpcBootstrappedNewBlock(RpcBootstrappedNewBlockAction { .. }) => {
            let bootstrapped = &store.state.get().rpc.bootstrapped;
            let rpc_ids: Vec<_> = bootstrapped.requests.iter().cloned().collect();
            if let Some(BootstrapState {
                json,
                is_bootstrapped,
            }) = &bootstrapped.state
            {
                for rpc_id in &rpc_ids {
                    store
                        .service
                        .rpc()
                        .respond_stream(*rpc_id, Some(json.clone()));
                }
                if *is_bootstrapped {
                    store.dispatch(RpcBootstrappedDoneAction { rpc_ids });
                }
            }
        }
        Action::RpcBootstrappedDone(RpcBootstrappedDoneAction { rpc_ids }) => {
            for rpc_id in rpc_ids {
                store.service.rpc().respond_stream(*rpc_id, None);
            }
        }
        _ => {}
    }
}
