// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

//! This channel is used to transmit p2p networking messages between actors.

use std::net::SocketAddr;
use std::sync::Arc;

use riker::actors::*;

use tezos_messages::p2p::encoding::advertise::AdvertiseMessage;
use tezos_messages::p2p::encoding::metadata::MetadataMessage;
use tezos_messages::p2p::encoding::peer::PeerMessageResponse;

use crate::PeerId;

use super::peer::PeerRef;
use tezos_messages::p2p::encoding::version::NetworkVersion;

/// Peer has been bootstrapped.
#[derive(Clone, Debug)]
pub struct PeerBootstrapFailed {
    pub address: SocketAddr,
    /// List of potential peers to connect to. Is extracted from `Nack`.
    pub potential_peers_to_connect: Option<Vec<String>>,
}

/// We have received message from another peer
#[derive(Clone, Debug)]
pub struct PeerMessageReceived {
    pub peer: PeerRef,
    pub message: Arc<PeerMessageResponse>,
}

/// Network channel event message.
#[derive(Clone, Debug)]
pub enum NetworkChannelMsg {
    /// Events
    PeerBootstrapped(Arc<PeerId>, Arc<MetadataMessage>, Arc<NetworkVersion>),
    PeerBlacklisted(Arc<PeerId>),
    PeerMessageReceived(PeerMessageReceived),
    PeerStalled(Arc<ActorUri>),
    /// Commands (dedicated to peer_manager)
    /// TODO: refactor/extract them directly to peer_manager outside of the network_channel
    BlacklistPeer(Arc<PeerId>, String),
    ProcessAdvertisedPeers(Arc<PeerId>, AdvertiseMessage),
    SendBootstrapPeers(Arc<PeerId>),
    ProcessFailedBootstrapAddress(PeerBootstrapFailed),
}

impl From<PeerMessageReceived> for NetworkChannelMsg {
    fn from(msg: PeerMessageReceived) -> Self {
        NetworkChannelMsg::PeerMessageReceived(msg)
    }
}

/// Represents various topics
pub enum NetworkChannelTopic {
    /// Events generated from networking layer
    NetworkEvents,
    /// Commands generated from other layers for network layer
    NetworkCommands,
}

impl From<NetworkChannelTopic> for Topic {
    fn from(evt: NetworkChannelTopic) -> Self {
        match evt {
            NetworkChannelTopic::NetworkEvents => Topic::from("network.events"),
            NetworkChannelTopic::NetworkCommands => Topic::from("network.commands"),
        }
    }
}

/// This struct represents network bus where all network events must be published.
pub struct NetworkChannel(Channel<NetworkChannelMsg>);

pub type NetworkChannelRef = ChannelRef<NetworkChannelMsg>;

impl NetworkChannel {
    pub fn actor(fact: &impl ActorRefFactory) -> Result<NetworkChannelRef, CreateError> {
        fact.actor_of::<NetworkChannel>(NetworkChannel::name())
    }

    fn name() -> &'static str {
        "network-event-channel"
    }
}

type ChannelCtx<Msg> = Context<ChannelMsg<Msg>>;

impl ActorFactory for NetworkChannel {
    fn create() -> Self {
        NetworkChannel(Channel::default())
    }
}

impl Actor for NetworkChannel {
    type Msg = ChannelMsg<NetworkChannelMsg>;

    fn pre_start(&mut self, ctx: &ChannelCtx<NetworkChannelMsg>) {
        self.0.pre_start(ctx);
    }

    fn recv(
        &mut self,
        ctx: &ChannelCtx<NetworkChannelMsg>,
        msg: ChannelMsg<NetworkChannelMsg>,
        sender: Sender,
    ) {
        self.0.receive(ctx, msg, sender);
    }
}
