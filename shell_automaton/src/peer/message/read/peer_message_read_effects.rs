// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::net::SocketAddr;

use crypto::hash::BlockHash;
use networking::network_channel::PeerMessageReceived;
use storage::BlockHeaderWithHash;
use tezos_messages::p2p::binary_message::{BinaryRead, MessageHash};
use tezos_messages::p2p::encoding::block_header::BlockHeader;
use tezos_messages::p2p::encoding::peer::{PeerMessage, PeerMessageResponse};
use tezos_messages::p2p::encoding::prelude::AdvertiseMessage;

use crate::bootstrap::{
    BootstrapPeerBlockHeaderGetSuccessAction, BootstrapPeerBlockOperationsReceivedAction,
    BootstrapPeerCurrentBranchReceivedAction,
};
use crate::peer::binary_message::read::PeerBinaryMessageReadInitAction;
use crate::peer::message::read::PeerMessageReadErrorAction;
use crate::peer::message::write::PeerMessageWriteInitAction;
use crate::peer::remote_requests::block_header_get::PeerRemoteRequestsBlockHeaderGetEnqueueAction;
use crate::peer::remote_requests::block_operations_get::PeerRemoteRequestsBlockOperationsGetEnqueueAction;
use crate::peer::remote_requests::current_branch_get::PeerRemoteRequestsCurrentBranchGetInitAction;
use crate::peer::{Peer, PeerCurrentHeadUpdateAction};
use crate::peers::add::multi::PeersAddMultiAction;
use crate::peers::graylist::PeersGraylistAddressAction;
use crate::service::actors_service::{ActorsMessageTo, ActorsService};
use crate::service::{RandomnessService, Service, StatisticsService};
use crate::{Action, ActionId, ActionWithMeta, State, Store};

use super::{PeerMessageReadInitAction, PeerMessageReadSuccessAction};

fn stats_message_received(
    state: &State,
    stats_service: Option<&mut StatisticsService>,
    message: &PeerMessage,
    address: SocketAddr,
    action_id: ActionId,
) {
    stats_service.map(|stats| {
        let time: u64 = action_id.into();
        let pending_block_header_requests = &state.peers.pending_block_header_requests;
        let node_id = state
            .peers
            .get(&address)
            .and_then(Peer::public_key_hash)
            .cloned();

        match message {
            PeerMessage::CurrentHead(m) => {
                m.current_block_header()
                    .message_typed_hash()
                    .map(|b: BlockHash| {
                        let block_header = m.current_block_header();
                        stats.block_new(
                            b.clone(),
                            block_header.level(),
                            block_header.timestamp(),
                            block_header.validation_pass(),
                            time,
                            Some(address),
                            node_id,
                        );
                    })
                    .unwrap_or(());
            }
            PeerMessage::BlockHeader(m) => m
                .block_header()
                .message_typed_hash()
                .map(|b: BlockHash| {
                    let block_header = m.block_header();
                    stats.block_new(
                        b.clone(),
                        block_header.level(),
                        block_header.timestamp(),
                        block_header.validation_pass(),
                        time,
                        Some(address),
                        node_id,
                    );
                    if let Some(time) = pending_block_header_requests.get(&b) {
                        stats.block_header_download_start(&b, *time);
                    }
                    stats.block_header_download_end(&b, time);
                })
                .unwrap_or(()),
            PeerMessage::OperationsForBlocks(m) => {
                let block_hash = m.operations_for_block().block_hash();
                stats.block_operations_download_end(block_hash, time);
            }
            PeerMessage::GetOperationsForBlocks(m) => {
                for gofb in m.get_operations_for_blocks() {
                    stats.block_get_operations_recv(
                        gofb.block_hash(),
                        time,
                        address,
                        node_id.as_ref(),
                        gofb.validation_pass(),
                    );
                }
            }
            _ => {}
        }
    });
}

pub fn peer_message_read_effects<S>(store: &mut Store<S>, action: &ActionWithMeta)
where
    S: Service,
{
    match &action.action {
        Action::PeerMessageReadInit(content) => {
            store.dispatch(PeerBinaryMessageReadInitAction {
                address: content.address,
            });
        }
        Action::PeerBinaryMessageReadReady(content) => {
            match store.state().peers.get(&content.address) {
                Some(peer) => match peer.status.as_handshaked() {
                    Some(_handshaked) => (),
                    None => return,
                },
                None => return,
            };

            match PeerMessageResponse::from_bytes(&content.message) {
                Ok(mut message) => {
                    // Set size hint to unencrypted encoded message size.
                    // Maybe we should set encrypted size instead? Since
                    // that's the actual size of data transmitted.
                    message.set_size_hint(content.message.len());

                    store.dispatch(PeerMessageReadSuccessAction {
                        address: content.address,
                        message: message.into(),
                    });
                }
                Err(err) => {
                    store.dispatch(PeerMessageReadErrorAction {
                        address: content.address,
                        error: err.into(),
                    });
                }
            }
        }
        Action::PeerMessageReadSuccess(content) => {
            store
                .service()
                .actors()
                .send(ActorsMessageTo::PeerMessageReceived(PeerMessageReceived {
                    peer_address: content.address,
                    message: content.message.clone(),
                }));

            match &content.message.message() {
                PeerMessage::Bootstrap => {
                    let potential_peers =
                        store.state.get().peers.potential_iter().collect::<Vec<_>>();
                    let advertise_peers = store
                        .service
                        .randomness()
                        .choose_potential_peers_for_advertise(&potential_peers);
                    store.dispatch(PeerMessageWriteInitAction {
                        address: content.address,
                        message: PeerMessageResponse::from(AdvertiseMessage::new(advertise_peers))
                            .into(),
                    });
                }
                PeerMessage::Advertise(msg) => {
                    store.dispatch(PeersAddMultiAction {
                        addresses: msg.id().iter().filter_map(|x| x.parse().ok()).collect(),
                    });
                }
                PeerMessage::GetCurrentBranch(msg) => {
                    if msg.chain_id != store.state().config.chain_id {
                        // TODO: log
                        return;
                    }
                    if !store.dispatch(PeerRemoteRequestsCurrentBranchGetInitAction {
                        address: action.address,
                    }) {
                        let state = store.state();
                        let current = state
                            .peers
                            .get_handshaked(&action.address)
                            .map(|p| &p.remote_requests.current_branch_get);
                        slog::debug!(&state.log, "Peer - Too many GetCurrentBranch requests!";
                                    "peer" => format!("{}", action.address),
                                    "current" => format!("{:?}", current));
                    }
                }
                PeerMessage::CurrentHead(msg) => {
                    if msg.chain_id() != &store.state().config.chain_id {
                        return;
                    }
                    update_peer_current_head(
                        store,
                        action.address,
                        msg.current_block_header().clone(),
                    );
                }
                PeerMessage::CurrentBranch(msg) => {
                    if msg.chain_id() != &store.state().config.chain_id {
                        return;
                    }
                    update_peer_current_head(
                        store,
                        action.address,
                        msg.current_branch().current_head().clone(),
                    );
                    store.dispatch(BootstrapPeerCurrentBranchReceivedAction {
                        peer: action.address,
                        current_branch: msg.current_branch().clone(),
                    });
                }
                PeerMessage::GetBlockHeaders(msg) => {
                    for block_hash in msg.get_block_headers() {
                        if !store.dispatch(PeerRemoteRequestsBlockHeaderGetEnqueueAction {
                            address: action.address,
                            block_hash: block_hash.clone(),
                        }) {
                            let state = store.state.get();
                            slog::debug!(&state.log, "Peer - Too many block header requests!";
                                "peer" => format!("{}", action.address),
                                "current_requested_block_headers_len" => msg.get_block_headers().len());
                            break;
                        }
                    }
                }
                PeerMessage::GetOperationsForBlocks(msg) => {
                    for key in msg.get_operations_for_blocks() {
                        if !store.dispatch(PeerRemoteRequestsBlockOperationsGetEnqueueAction {
                            address: action.address,
                            key: key.into(),
                        }) {
                            let state = store.state.get();
                            slog::debug!(&state.log, "Peer - Too many block operations requests!";
                                "peer" => format!("{}", action.address),
                                "current_requested_block_operations_len" => msg.get_operations_for_blocks().len());
                            break;
                        }
                    }
                }
                PeerMessage::BlockHeader(msg) => {
                    let state = store.state.get();
                    let block = match BlockHeaderWithHash::new(msg.block_header().clone()) {
                        Ok(v) => v,
                        Err(err) => {
                            slog::warn!(&state.log, "Failed to hash BlockHeader";
                                "peer" => format!("{}", action.address),
                                "peer_pkh" => format!("{:?}", state.peer_public_key_hash_b58check(action.address)),
                                "block_header" => format!("{:?}", msg.block_header()),
                                "error" => format!("{:?}", err));
                            store.dispatch(PeersGraylistAddressAction {
                                address: content.address,
                            });
                            return;
                        }
                    };
                    if let Some((_, p)) = state.bootstrap.peer_interval(action.address, |p| {
                        p.current.is_pending_block_hash_eq(&block.hash)
                    }) {
                        if !p.current.is_pending_block_level_eq(block.header.level()) {
                            slog::warn!(&state.log, "BlockHeader level didn't match expected level for requested block hash";
                                "peer" => format!("{}", action.address),
                                "peer_pkh" => format!("{:?}", state.peer_public_key_hash_b58check(action.address)),
                                "block" => format!("{:?}", block),
                                "expected_level" => format!("{:?}", p.current.block_level()));
                            store.dispatch(PeersGraylistAddressAction {
                                address: content.address,
                            });
                            return;
                        }
                        store.dispatch(BootstrapPeerBlockHeaderGetSuccessAction {
                            peer: action.address,
                            block,
                        });
                    } else {
                        // dbg!(&state.bootstrap);
                        dbg!(state
                            .bootstrap
                            .peer_intervals()
                            .and_then(|intervals| intervals
                                .iter()
                                .find(|p| p.current.block_hash() == Some(&block.hash))));
                        slog::warn!(&state.log, "Received unexpected BlockHeader from peer";
                            "peer" => format!("{}", action.address),
                            "peer_pkh" => format!("{:?}", state.peer_public_key_hash_b58check(action.address)),
                            "block" => format!("{:?}", &block),
                            "expected" => format!("{:?}", state.bootstrap.peer_interval(action.address, |p| p.current.is_pending())));
                        // TODO(zura): fix us requesting same block header multiple times.
                        // store.dispatch(PeersGraylistAddressAction {
                        //     address: action.address,
                        // });
                    }
                }
                PeerMessage::OperationsForBlocks(msg) => {
                    store.dispatch(BootstrapPeerBlockOperationsReceivedAction {
                        peer: content.address,
                        message: msg.clone(),
                    });
                }
                _ => {}
            }

            stats_message_received(
                store.state.get(),
                store.service.statistics(),
                content.message.message(),
                content.address,
                action.id,
            );

            // try to read next message.
            store.dispatch(PeerMessageReadInitAction {
                address: content.address,
            });
        }
        Action::PeerMessageReadError(content) => {
            store.dispatch(PeersGraylistAddressAction {
                address: content.address,
            });
        }
        _ => {}
    }
}
pub fn update_peer_current_head<S>(
    store: &mut Store<S>,
    address: SocketAddr,
    block_header: BlockHeader,
) where
    S: Service,
{
    match BlockHeaderWithHash::new(block_header) {
        Ok(current_head) => {
            store.dispatch(PeerCurrentHeadUpdateAction {
                address,
                current_head,
            });
        }
        // TODO(zura): log
        Err(_) => return,
    }
}
