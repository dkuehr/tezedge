// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::convert::TryInto;

use crypto::{blake2b, hash::BlockHash};
use slog::{debug, error};
use tezos_messages::p2p::binary_message::{BinaryWrite, MessageHash};

use crate::{
    mempool::{BlockAppliedAction, MempoolOperationDecodedAction},
    prechecker::{Applied, PrecheckerEndorsementValidationRefusedAction, Refused},
    rights::{
        EndorsingRightsKey, RightsEndorsingRightsErrorAction, RightsEndorsingRightsReadyAction,
        RightsGetEndorsingRightsAction,
    },
    Action, ActionWithMeta, Service, Store,
};

use super::{
    EndorsementValidationError, Key, OperationDecodedContents, PrecheckerDecodeOperationAction,
    PrecheckerEndorsementValidationAppliedAction, PrecheckerEndorsingRightsReadyAction,
    PrecheckerError, PrecheckerErrorAction, PrecheckerGetEndorsingRightsAction,
    PrecheckerOperation, PrecheckerOperationDecodedAction, PrecheckerOperationState,
    PrecheckerPrecheckOperationInitAction, PrecheckerPrecheckOperationRequestAction,
    PrecheckerPrecheckOperationResponseAction, PrecheckerProtocolNeededAction,
    PrecheckerValidateEndorsementAction,
};

pub fn prechecker_effects<S>(store: &mut Store<S>, action: &ActionWithMeta)
where
    S: Service,
{
    let prechecker_state = &store.state.get().prechecker;
    let prechecker_state_operations = &prechecker_state.operations;
    let log = &store.state.get().log;
    match &action.action {
        // debug only
        Action::BlockApplied(BlockAppliedAction {
            block,
            is_bootstrapped,
            ..
        }) if *is_bootstrapped => {
            let block_hash = if let Ok(v) = block.message_typed_hash::<BlockHash>() {
                v
            } else {
                return;
            };
            debug!(log, "New block applied"; "block_hash" => block_hash.to_base58_check());

            for (key, op) in prechecker_state
                .non_terminals()
                .filter(|(_, state)| state.operation.branch() != &block_hash)
            {
                debug!(log, "Prevalidator operation still unprocessed";
                           "operation" => key.operation.to_base58_check(), "state" => op.state.as_ref(), "block_hash" => op.block_hash().to_base58_check());
            }

            for (key, op) in prechecker_state_operations {
                match &op.state {
                    PrecheckerOperationState::Refused { error, .. } => {
                        debug!(log, "Prevalidator refused operation";
                           "operation" => key.operation.to_base58_check(), "error" => error.to_string(), "block_hash" => op.block_hash().to_base58_check())
                    }
                    _ => (),
                }
            }
        }

        Action::PrecheckerPrecheckOperationRequest(PrecheckerPrecheckOperationRequestAction {
            operation,
        }) => {
            let binary_encoding = match operation.as_bytes() {
                Ok(bytes) => bytes,
                Err(err) => {
                    store.dispatch(PrecheckerPrecheckOperationResponseAction::error(err));
                    return;
                }
            };

            let hash = match blake2b::digest_256(&binary_encoding) {
                Ok(hash) => hash,
                Err(err) => {
                    store.dispatch(PrecheckerPrecheckOperationResponseAction::error(err));
                    return;
                }
            };
            let key = match hash.try_into() {
                Ok(hash) => Key { operation: hash },
                Err(err) => {
                    store.dispatch(PrecheckerPrecheckOperationResponseAction::error(err));
                    return;
                }
            };
            store.dispatch(PrecheckerPrecheckOperationInitAction {
                key,
                operation: operation.clone(),
                operation_binary_encoding: binary_encoding,
            });
        }
        Action::PrecheckerPrecheckOperationInit(PrecheckerPrecheckOperationInitAction {
            key,
            ..
        }) => {
            match prechecker_state_operations.get(key).map(|op| &op.state) {
                Some(PrecheckerOperationState::Init { .. }) => {
                    store.dispatch(PrecheckerDecodeOperationAction { key: key.clone() });
                }
                Some(PrecheckerOperationState::Applied { protocol_data }) => {
                    let action = PrecheckerEndorsementValidationAppliedAction {
                        key: key.clone(),
                        protocol_data: protocol_data.clone(),
                    };
                    store.dispatch(action);
                }
                Some(PrecheckerOperationState::Error { error, .. }) => {
                    let error = error.clone();
                    store.dispatch(PrecheckerErrorAction::new(key.clone(), error));
                }
                _ => (),
            };
        }
        Action::PrecheckerDecodeOperation(PrecheckerDecodeOperationAction { key }) => {
            if let Some(PrecheckerOperation {
                operation_binary_encoding,
                state: PrecheckerOperationState::PendingContentDecoding,
                ..
            }) = prechecker_state_operations.get(key)
            {
                // TODO use proper protocol to parse operation
                match OperationDecodedContents::parse(&operation_binary_encoding) {
                    Ok(contents) => {
                        store.dispatch(PrecheckerOperationDecodedAction {
                            key: key.clone(),
                            contents,
                        });
                    }
                    Err(err) => {
                        store.dispatch(PrecheckerErrorAction::new(key.clone(), err));
                    }
                }
            }
        }
        Action::PrecheckerOperationDecoded(PrecheckerOperationDecodedAction { key, .. }) => {
            if let Some(PrecheckerOperationState::DecodedContentReady {
                operation_decoded_contents,
                ..
            }) = prechecker_state_operations.get(key).map(|op| &op.state)
            {
                let is_endorsement = operation_decoded_contents.is_endorsement();
                let protocol_data = operation_decoded_contents.as_json();
                let protocol_data_clone = protocol_data.clone();

                store.dispatch(MempoolOperationDecodedAction {
                    operation: key.operation.clone(),
                    protocol_data,
                });

                if !is_endorsement {
                    store.dispatch(PrecheckerProtocolNeededAction {
                        key: key.clone(),
                        protocol_data: protocol_data_clone,
                    });
                } else {
                    store.dispatch(PrecheckerGetEndorsingRightsAction { key: key.clone() });
                }
            }
        }

        Action::PrecheckerGetEndorsingRights(PrecheckerGetEndorsingRightsAction { key }) => {
            if let Some(PrecheckerOperationState::PendingEndorsingRights {
                operation_decoded_contents,
                ..
            }) = prechecker_state_operations.get(key).map(|op| &op.state)
            {
                if let Some(current_block_hash) = &store.state.get().prechecker.latest_block_hash {
                    if let Some(level) = operation_decoded_contents.endorsement_level() {
                        let current_block_hash = current_block_hash.clone();
                        store.dispatch(RightsGetEndorsingRightsAction {
                            key: EndorsingRightsKey {
                                current_block_hash,
                                level: Some(level),
                            },
                        });
                    }
                }
            }
        }
        Action::RightsEndorsingRightsReady(RightsEndorsingRightsReadyAction {
            key: EndorsingRightsKey {
                level: Some(level), ..
            },
            endorsing_rights,
        }) => {
            for key in prechecker_state_operations
                .iter()
                .filter_map(|(key, state)| {
                    if let PrecheckerOperationState::PendingEndorsingRights {
                        operation_decoded_contents,
                        ..
                    } = &state.state
                    {
                        if operation_decoded_contents
                            .endorsement_level()
                            .map(|l| l == *level)
                            .unwrap_or(false)
                        {
                            Some(key)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .cloned()
                .collect::<Vec<_>>()
            {
                store.dispatch(PrecheckerEndorsingRightsReadyAction {
                    key,
                    endorsing_rights: endorsing_rights.clone(),
                });
            }
        }
        Action::RightsEndorsingRightsError(RightsEndorsingRightsErrorAction {
            key:
                EndorsingRightsKey {
                    current_block_hash,
                    level: None,
                },
            error,
        }) => {
            for key in prechecker_state_operations
                .iter()
                .filter_map(|(key, state)| {
                    if let PrecheckerOperationState::PendingEndorsingRights { .. } = state.state {
                        if state.operation.branch() == current_block_hash {
                            Some(key)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .cloned()
                .collect::<Vec<_>>()
            {
                store.dispatch(PrecheckerErrorAction::new(key, error.clone()));
            }
        }
        Action::PrecheckerEndorsingRightsReady(PrecheckerEndorsingRightsReadyAction {
            key,
            ..
        }) => {
            if let Some(PrecheckerOperationState::EndorsingRightsReady { .. }) =
                prechecker_state_operations.get(key).map(|op| &op.state)
            {
                store.dispatch(PrecheckerValidateEndorsementAction { key: key.clone() });
            }
        }
        Action::PrecheckerValidateEndorsement(PrecheckerValidateEndorsementAction { key }) => {
            if let Some(PrecheckerOperation {
                operation_binary_encoding,
                state:
                    PrecheckerOperationState::PendingOperationPrechecking {
                        operation_decoded_contents,
                        endorsing_rights,
                    },
                ..
            }) = prechecker_state_operations.get(key)
            {
                let block_hash = operation_decoded_contents.branch();
                let chain_id = &store.state.get().config.chain_id;
                use super::EndorsementValidator;
                let validation_result = match operation_decoded_contents {
                    OperationDecodedContents::Proto010(operation) => operation
                        .validate_endorsement(
                            operation_binary_encoding,
                            chain_id,
                            block_hash,
                            endorsing_rights,
                            log,
                        ),
                };

                match validation_result {
                    Ok(Applied { protocol_data }) => {
                        store.dispatch(PrecheckerEndorsementValidationAppliedAction {
                            key: key.clone(),
                            protocol_data: protocol_data.clone(),
                        });
                    }
                    Err(Refused {
                        protocol_data,
                        error: EndorsementValidationError::UnsupportedPublicKey,
                    }) => {
                        store.dispatch(PrecheckerProtocolNeededAction {
                            key: key.clone(),
                            protocol_data: protocol_data.clone(),
                        });
                    }
                    Err(Refused {
                        protocol_data,
                        error,
                    }) => {
                        store.dispatch(PrecheckerEndorsementValidationRefusedAction {
                            key: key.clone(),
                            protocol_data,
                            error,
                        });
                    }
                };
            }
        }
        Action::PrecheckerProtocolNeeded(PrecheckerProtocolNeededAction { key, protocol_data }) => {
            if let Some(PrecheckerOperationState::ProtocolNeeded { .. }) =
                prechecker_state_operations.get(key).map(|op| &op.state)
            {
                store.dispatch(PrecheckerPrecheckOperationResponseAction::prevalidate(
                    &key.operation,
                    protocol_data,
                ));
            }
        }

        Action::PrecheckerEndorsementValidationApplied(
            PrecheckerEndorsementValidationAppliedAction { key, protocol_data },
        ) => {
            if let Some(PrecheckerOperationState::Applied { .. }) =
                prechecker_state_operations.get(key).map(|op| &op.state)
            {
                store.dispatch(PrecheckerPrecheckOperationResponseAction::valid(
                    &key.operation,
                    protocol_data.clone(),
                ));
            }
        }
        Action::PrecheckerEndorsementValidationRefused(
            PrecheckerEndorsementValidationRefusedAction { key, .. },
        ) => {
            if let Some(PrecheckerOperation {
                state:
                    PrecheckerOperationState::Refused {
                        protocol_data,
                        error,
                    },
                ..
            }) = prechecker_state_operations.get(key)
            {
                let action = PrecheckerPrecheckOperationResponseAction::reject(
                    &key.operation,
                    protocol_data.clone(),
                    serde_json::to_string(error).unwrap_or("<unserialized>".to_string()),
                );
                store.dispatch(action);
            }
        }
        Action::PrecheckerError(PrecheckerErrorAction { key, error }) => {
            if let Some(PrecheckerOperationState::Error { .. }) =
                prechecker_state_operations.get(key).map(|op| &op.state)
            {
                match error {
                    PrecheckerError::EndorsingRights(err) => {
                        error!(log, "Getting endorsing rights failed"; "operation" => key.to_string(), "error" => err.to_string());
                        store.dispatch(PrecheckerPrecheckOperationResponseAction::error(
                            err.clone(),
                        ));
                    }
                    PrecheckerError::OperationContentsDecode(err) => {
                        store.dispatch(PrecheckerPrecheckOperationResponseAction::error(
                            err.clone(),
                        ));
                    }
                }
            }
        }
        _ => (),
    }
}
