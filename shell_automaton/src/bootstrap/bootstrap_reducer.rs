// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::collections::VecDeque;

use crypto::seeded_step::{Seed, Step};
use tezos_messages::p2p::{binary_message::MessageHash, encoding::block_header::Level};

use crate::{Action, ActionWithMeta, State};

use super::{
    BlockWithDownloadedHeader, BootstrapBlockOperationGetState, BootstrapState,
    PeerBlockOperationsGetState, PeerIntervalState,
};
pub fn bootstrap_reducer(state: &mut State, action: &ActionWithMeta) {
    match &action.action {
        Action::BootstrapInit(_) => {
            state.bootstrap = BootstrapState::Init {
                time: action.time_as_nanos(),
            };
        }
        Action::BootstrapPeersConnectPending(_) => {
            state.bootstrap = BootstrapState::PeersConnectPending {
                time: action.time_as_nanos(),
            };
        }
        Action::BootstrapPeersConnectSuccess(_) => {
            state.bootstrap = BootstrapState::PeersConnectSuccess {
                time: action.time_as_nanos(),
            };
        }
        Action::BootstrapPeersMainBranchFindPending(_) => {
            state.bootstrap = BootstrapState::PeersMainBranchFindPending {
                time: action.time_as_nanos(),
                peer_branches: Default::default(),
                block_supporters: Default::default(),
            };
        }
        Action::BootstrapPeerCurrentBranchReceived(content) => {
            match &mut state.bootstrap {
                BootstrapState::PeersMainBranchFindPending {
                    peer_branches,
                    block_supporters,
                    ..
                } => {
                    peer_branches.insert(content.peer, content.current_branch.clone());
                    let level = content.current_branch.current_head().level();

                    IntoIterator::into_iter([
                        Ok((
                            level - 1,
                            content.current_branch.current_head().predecessor().clone(),
                        )),
                        content
                            .current_branch
                            .current_head()
                            .message_typed_hash()
                            .map(|hash| (level, hash)),
                    ])
                    .filter_map(Result::ok)
                    .for_each(|(level, block_hash)| {
                        block_supporters
                            .entry(block_hash)
                            .or_insert((level, Default::default()))
                            .1
                            .insert(content.peer);
                    });
                }
                // TODO(zura): handle current branches when received after
                // main chain is found.
                _ => {}
            }
        }
        Action::BootstrapPeersMainBranchFindSuccess(_) => match &mut state.bootstrap {
            BootstrapState::PeersMainBranchFindPending { peer_branches, .. } => {
                let peer_branches = std::mem::take(peer_branches);
                if let Some(main_block) = state
                    .bootstrap
                    .main_block(state.config.peers_bootstrapped_min)
                {
                    state.bootstrap = dbg!(BootstrapState::PeersMainBranchFindSuccess {
                        time: action.time_as_nanos(),

                        main_block,
                        peer_branches,
                    });
                }
            }
            _ => {}
        },
        Action::BootstrapPeersBlockHeadersGetPending(_) => {
            let current_head = match state.current_head.get() {
                Some(v) => v,
                None => return,
            };

            if let BootstrapState::PeersMainBranchFindSuccess {
                main_block,
                peer_branches,
                ..
            } = &mut state.bootstrap
            {
                let main_block = main_block.clone();
                let mut peer_intervals = vec![];
                let missing_levels_count = main_block.0 - current_head.header.level();

                for (peer, branch) in std::mem::take(peer_branches) {
                    // Calculate step for branch to associate block hashes
                    // in the branch with expected levels.
                    let peer_pkh = match state.peer_public_key_hash(peer) {
                        Some(v) => v,
                        None => continue,
                    };
                    let seed = Seed::new(peer_pkh, &state.config.identity.peer_id);
                    let block_hash = if main_block.0 == branch.current_head().level() {
                        main_block.1.clone()
                    } else {
                        match branch.current_head().message_typed_hash() {
                            Ok(v) => v,
                            Err(_) => continue,
                        }
                    };
                    let mut step = Step::init(&seed, &block_hash);

                    let mut level = branch.current_head().level();

                    if level == main_block.0 {
                        peer_intervals.push(PeerIntervalState {
                            peer,
                            downloaded: vec![],
                            current: Some((level, main_block.1.clone())),
                        });
                    }
                    for block_hash in branch.history() {
                        level -= step.next_step();
                        if level <= current_head.header.level() {
                            break;
                        }
                        peer_intervals.push(PeerIntervalState {
                            peer,
                            downloaded: vec![],
                            current: Some((level, block_hash.clone())),
                        });
                    }
                }

                let cmp_levels = |a: &PeerIntervalState, b: &PeerIntervalState| {
                    a.current
                        .as_ref()
                        .map(|(l, _)| l)
                        .cmp(&b.current.as_ref().map(|(l, _)| l))
                };
                peer_intervals.sort_by(|a, b| cmp_levels(a, b));
                peer_intervals.dedup_by(|a, b| cmp_levels(a, b).is_eq());

                state.bootstrap = dbg!(BootstrapState::PeersBlockHeadersGetPending {
                    time: action.time_as_nanos(),
                    main_chain_last_level: main_block.0,
                    main_chain: VecDeque::with_capacity(missing_levels_count.max(0) as usize),
                    peer_intervals,
                });
            }
        }
        Action::BootstrapPeerBlockHeaderReceived(content) => {
            let current_head = match state.current_head.get() {
                Some(v) => v,
                None => return,
            };
            let index = match state
                .bootstrap
                .peer_interval_by_level_pos(content.peer, content.block.header.level())
            {
                Some(v) => v,
                None => return,
            };
            if let BootstrapState::PeersBlockHeadersGetPending {
                main_chain_last_level,
                main_chain,
                peer_intervals,
                ..
            } = &mut state.bootstrap
            {
                peer_intervals[index].current = Some((
                    content.block.header.level() - 1,
                    content.block.header.predecessor().clone(),
                ));
                peer_intervals[index].downloaded.push((
                    content.block.header.level(),
                    content.block.hash.clone(),
                    content.block.header.validation_pass(),
                    content.block.header.operations_hash().clone(),
                ));

                // check if we have finished downloading interval or
                // if this interval reached predecessor. So that
                // pred_interval_level == current_interval_next_level.
                if index > 0 {
                    let pred_index = index - 1;
                    let pred = &peer_intervals[pred_index];
                    match pred
                        .downloaded
                        .first()
                        .map(|(l, h, ..)| (l, h))
                        .or(pred.current.as_ref().map(|(l, h)| (l, h)))
                    {
                        Some((pred_level, pred_hash)) => {
                            if pred_level + 1 == content.block.header.level() {
                                if content.block.header.predecessor() != pred_hash {
                                    slog::warn!(&state.log, "Predecessor hash mismatch!";
                                        "block_header" => format!("{:?}", content.block),
                                        "pred_interval" => format!("{:?}", peer_intervals.get(pred_index - 1)),
                                        "interval" => format!("{:?}", pred),
                                        "next_interval" => format!("{:?}", peer_intervals[index]));
                                    todo!("log and remove pred interval, update `index -= 1`, somehow trigger blacklisting a peer.");
                                } else {
                                    // We finished interval.
                                    peer_intervals[index].current = None;
                                }
                            }
                        }
                        None => {
                            slog::warn!(&state.log, "Found empty block header download interval when bootstrapping. Should not happen!";
                                "pred_interval" => format!("{:?}", peer_intervals.get(pred_index - 1)),
                                "interval" => format!("{:?}", pred),
                                "next_interval" => format!("{:?}", peer_intervals[index]));
                            peer_intervals.remove(pred_index);
                        }
                    };
                } else {
                    let pred_level = content.block.header.level() - 1;
                    if pred_level <= current_head.header.level() {
                        peer_intervals[index].current = None;
                    }
                }

                loop {
                    let main_chain_next_level = *main_chain_last_level - main_chain.len() as Level;
                    let interval = match peer_intervals.last_mut() {
                        Some(v) => v,
                        None => break,
                    };
                    let first_downloaded_level = match interval.downloaded.first() {
                        Some(v) => v.0,
                        None => break,
                    };
                    if first_downloaded_level != main_chain_next_level {
                        break;
                    }
                    for (_, block_hash, validation_pass, operations_hash) in
                        interval.downloaded.drain(..)
                    {
                        main_chain.push_front(BlockWithDownloadedHeader {
                            peer: interval.peer,
                            block_hash,
                            validation_pass,
                            operations_hash,
                        });
                    }
                    // interval finished so we can remove it.
                    if interval.current.is_none() {
                        peer_intervals.pop();
                    }
                }
            }
        }
        Action::BootstrapPeersBlockHeadersGetSuccess(_) => match &mut state.bootstrap {
            BootstrapState::PeersBlockHeadersGetPending {
                main_chain_last_level,
                main_chain,
                ..
            } => {
                state.bootstrap = BootstrapState::PeersBlockHeadersGetSuccess {
                    time: action.time_as_nanos(),
                    chain_last_level: *main_chain_last_level,
                    chain: std::mem::take(main_chain),
                };
            }
            _ => {}
        },
        Action::BootstrapPeersBlockOperationsGetPending(_) => match &mut state.bootstrap {
            BootstrapState::PeersBlockHeadersGetSuccess {
                chain_last_level,
                chain,
                ..
            } => {
                state.bootstrap = BootstrapState::PeersBlockOperationsGetPending {
                    time: action.time_as_nanos(),
                    last_level: *chain_last_level,
                    queue: std::mem::take(chain),
                    pending: Default::default(),
                };
            }
            _ => {}
        },
        Action::BootstrapPeerBlockOperationsGetPending(_) => match &mut state.bootstrap {
            BootstrapState::PeersBlockOperationsGetPending {
                queue,
                pending,
                last_level,
                ..
            } => {
                let next_block = match queue.pop_front() {
                    Some(v) => v,
                    None => return,
                };
                let next_block_level = *last_level - queue.len() as i32;

                pending
                    .entry(next_block.block_hash.clone())
                    .or_insert(BootstrapBlockOperationGetState {
                        block_level: next_block_level,
                        validation_pass: next_block.validation_pass,
                        operations_hash: next_block.operations_hash.clone(),
                        peers: Default::default(),
                    })
                    .peers
                    .insert(
                        next_block.peer,
                        PeerBlockOperationsGetState::Pending {
                            time: action.time_as_nanos(),
                            operations: vec![None; next_block.validation_pass as usize],
                        },
                    );
            }
            _ => {}
        },
        Action::BootstrapPeerBlockOperationsReceived(content) => match &mut state.bootstrap {
            BootstrapState::PeersBlockOperationsGetPending { pending, .. } => {
                pending
                    .get_mut(content.message.operations_for_block().block_hash())
                    .and_then(|b| b.peers.get_mut(&content.peer))
                    .and_then(|p| match p {
                        PeerBlockOperationsGetState::Pending { operations, .. } => Some(operations),
                        _ => None,
                    })
                    .and_then(|operations| {
                        operations.get_mut(
                            content.message.operations_for_block().validation_pass() as usize
                        )
                    })
                    .map(|v| *v = Some(content.message.clone()));
            }
            _ => {}
        },
        Action::BootstrapPeerBlockOperationsGetSuccess(content) => match &mut state.bootstrap {
            BootstrapState::PeersBlockOperationsGetPending { pending, .. } => {
                pending
                    .get_mut(&content.block_hash)
                    .and_then(|b| b.peers.iter_mut().find(|(_, p)| p.is_complete()))
                    .map(|(_, p)| match p {
                        PeerBlockOperationsGetState::Pending { operations, .. } => {
                            let operations = operations.drain(..).filter_map(|v| v).collect();
                            *p = PeerBlockOperationsGetState::Success {
                                time: action.time_as_nanos(),
                                operations,
                            };
                        }
                        _ => {}
                    });
            }
            _ => {}
        },
        Action::BootstrapScheduleBlockForApply(content) => match &mut state.bootstrap {
            BootstrapState::PeersBlockOperationsGetPending { pending, .. } => {
                pending.remove(&content.block_hash);
            }
            _ => {}
        },
        _ => {}
    }
}