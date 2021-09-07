//! Tezos protocol handshake procedure

#![feature(hash_set_entry)]

pub mod redux;
pub mod network;

//use anyhow::Result;

//use crypto::hash::CryptoboxPublicKeyHash;

//use tezos_messages::p2p::encoding::prelude::*;

//use redux_rs::{Store, Subscription};
//

/*
use std::{
    cmp,
    collections::{HashMap, VecDeque},
    io::{self, Read, Write},
    mem,
    net::SocketAddr,
    thread,
    time::Duration,
};

use crypto::{
    crypto_box::{PublicKey, SecretKey},
    nonce::Nonce,
    proof_of_work::ProofOfWork,
};
use mio::{
    net::{TcpListener, TcpStream},
    Events, Interest, Poll, Token,
};
use tezos_messages::p2p::{
    binary_message::BinaryWrite,
    encoding::{
        ack::{AckMessage, NackInfo, NackMotive},
        connection::ConnectionMessage,
        metadata::MetadataMessage,
        version::NetworkVersion,
    },
};

struct NodeState {
    addr: SocketAddr,
    public_key: PublicKey,
    secret_key: SecretKey,
    pow: ProofOfWork,
    peers: HashMap<usize, Peer>,
}

struct Peer {
    state: PeerHandshakeState,

    message_send_state: SendState,
    message_recv_state: RecvState,

    address: Option<SocketAddr>,
    public_key: Option<PublicKey>,
    local_nonce: Nonce,
    remote_nonce: Option<Nonce>,
}

enum PeerHandshakeState {
    ConnectionExchange(bool, bool),
    MetadataExchange(bool, bool),
    AckExchange(bool, bool),
    Complete,
    Error(String),
}

enum SendState {
    Idle,
    Sending {
        binary_message: Vec<u8>,
        next_chunk_start_pos: usize,
    },
}

impl SendState {
    fn start_sending_data(&mut self, binary_message: Vec<u8>) {
        if let Self::Sending {..} = self {
            impossible!("Already in sending state, data loss");
            return;
        }
        *self = Self::Sending {
            binary_message,
            next_chunk_start_pos: 0,
        }
    }

    fn send_next_chunk(&mut self, chunk_size: usize) {
        match self {
            Self::Idle => impossible!("Can't send next chunk in idle state"),
            Self::Sending {
                binary_message,
                next_chunk_start_pos,
            } => {
                if *next_chunk_start_pos + chunk_size > binary_message.len() {
                    impossible!("Trying to send more data than exist");
                    return;
                }
                *next_chunk_start_pos += chunk_size;
            }
        }
    }

    fn message_is_sent(&mut self) {
        match self {
            Self::Idle => impossible!("Message cannot be sent while in Idle state"),
            Self::Sending {
                binary_message,
                next_chunk_start_pos,
            } => {
                if next_chunk_start_pos != binary_message.len() {
                    impossible!("Message is not sent fully");
                    return;
                }
                *self = Self::Idle;
            }
        }
    }

    fn get_next_chunk(&self, chunk_max_size: usize) -> &[u8] {
        match self {
            Self::Idle => impossible!(&[]; "Message cannot be sent while in Idle state"),
            Self::Sending {
                binary_message,
                next_chunk_start_pos,
            } => {
                if *next_chunk_start_pos >= binary_message.len() {
                    impossible!("All message has been sent, cannot get next chunk");
                    return &[];
                }
                let next_chunk_end_pos =
                    cmp::min(*next_chunk_start_pos + chunk_max_size, binary_message.len());
                &binary_message[*next_chunk_start_pos..next_chunk_end_pos]
            }
        }
    }
}

pub const CONTENT_LENGTH_MAX: usize =
    tezos_messages::p2p::binary_message::CONTENT_LENGTH_MAX - crypto::crypto_box::BOX_ZERO_BYTES;

impl NodeState {
    fn new() -> Self {
        todo!()
    }

    fn get_peer(&self, peer_num: usize) -> Option<&Peer> {
        self.peers.get(&peer_num)
    }
}

impl Peer {
    fn new() -> Self {
        todo!()
    }
}

enum Action {
    Handshake(usize, HandshakeAction),
    Send(usize, SendAction),
    Network(usize, NetworkAction),
    Error(String),
}

enum SendAction {
    SendData(Vec<u8>),
    SendNextChunk,
    ChunkSent,
    Error(String),
    ChunkError,
}

enum HandshakeAction {
    ConnectionRecv(bool, ConnectionMessage),
    SendConnection(ConnectionMessage),
    ConnectionSent(ConnectionMessage),
    MetadataRecv(MetadataMessage),
    SendMetadata(MetadataMessage),
    MetadataSent(MetadataMessage),
    AckRecv,
    SendAck,
    AckSent,
    NackRecv(NackInfo),
    SendNack(NackInfo),
    NackSent(NackInfo),

    Disconnect(&'static str),
    Blacklist(&'static str),
    Nack(NackInfo, bool),
    /// Error occured locally.
    Error(String),

    Complete,
}

impl From<(usize, HandshakeAction)> for Action {
    fn from(source: (usize, HandshakeAction)) -> Self {
        Self::Handshake(source.0, source.1)
    }
}

impl From<(usize, SendAction)> for Action {
    fn from(source: (usize, SendAction)) -> Self {
        Self::Handshake(source.0, source.1)
    }
}



impl NodeState {
    fn blacklist_peer(&mut self, peer_num: usize) {
        todo!()
    }

    fn disconnect_peer(&mut self, peer_num: usize) {
        todo!()
    }

    fn blacklisted(&self, peer_num: usize, peer: &Peer) -> bool {
        todo!()
    }

    fn can_accept_more_connections(&self) -> bool {
        todo!()
    }

    fn update_local_data(&mut self, peer_num: usize, connection: ConnectionMessage) {
        todo!()
    }

    fn update_remote_data(&mut self, peer_num: usize, connection: ConnectionMessage) {
        todo!()
    }

    fn already_connected(&self, peer_num: usize, peer: &Peer) -> bool {
        todo!()
    }

    fn nack_peer(&mut self, peer_num: usize, motive: NackMotive, blacklist: bool) {
        todo!()
    }
}

impl State<Action> for NodeState {
    fn reduce(&mut self, action: Action) {
        match action {
            Action::Handshake(peer_num, action) => {
                let peer = self.peers.get_or_insert(peer_num, Peer::new());
                match action {
                    HandshakeAction::ConnectionRecv(incoming, connection) => {
                        peer.update_remote_data(connection);
                        match peer.state {
                            PeerHandshakeState::ConnectionExchange(false, ref mut recv) => {
                                *recv = true
                            } // send is pending, recv done
                            PeerHandshakeState::ConnectionExchange(_, _) => {
                                peer.state = PeerHandshakeState::MetadataExchange(false, false)
                            } // both send and recv are done, moving to metadata
                            _ => impossible!("Should not happen"),
                        }
                    }
                    HandshakeAction::SendConnection(connection) => {
                        peer.update_local_data(connection)
                    }
                    HandshakeAction::ConnectionSent => match peer.state {
                        PeerHandshakeState::ConnectionExchange(ref mut sent, false) => *sent = true,
                        PeerHandshakeState::ConnectionExchange(_, _) => {
                            peer.state = PeerHandshakeState::MetadataExchange(false, false)
                        }
                        _ => impossible!("Should not happen"),
                    },

                    HandshakeAction::MetadataRecv(_) => match peer.state {
                        PeerHandshakeState::MetadataExchange(false, ref mut recv) => *recv = true,
                        PeerHandshakeState::MetadataExchange(_, _) => {
                            peer.state = PeerHandshakeState::AckExchange(false, false)
                        }
                        _ => impossible!("Should not happen"),
                    },
                    HandshakeAction::SendMetadata(_) => (),
                    HandshakeAction::MetadataSent => match peer.state {
                        PeerHandshakeState::MetadataExchange(ref mut sent, false) => *sent = true,
                        PeerHandshakeState::MetadataExchange(_, _) => {
                            peer.state = PeerHandshakeState::AckExchange(false, false)
                        }
                        _ => impossible!("Should not happen"),
                    },

                    HandshakeAction::AckRecv => match peer.state {
                        PeerHandshakeState::AckExchange(false, ref mut recv) => *recv = true,
                        PeerHandshakeState::AckExchange(_, _) => {
                            peer.state = PeerHandshakeState::Complete
                        }
                        _ => impossible!("Should not happen"),
                    },
                    HandshakeAction::SendAck(_) => (),
                    HandshakeAction::AckSent => match peer.state {
                        PeerHandshakeState::AckExchange(ref mut sent, false) => *sent = true,
                        PeerHandshakeState::AckExchange(_, _) => {
                            peer.state = PeerHandshakeState::Complete
                        }
                        _ => impossible!("Should not happen"),
                    },

                    HandshakeAction::Blacklist(reason) => match peer.state {
                        PeerHandshakeState::Error(_) => impossible!("Should not happen"),
                        _ => {
                            peer.state =
                                PeerHandshakeState::Error(format!("blacklisted: {}", reason))
                        }
                    },
                    HandshakeAction::Disconnect(reason) => match peer.state {
                        PeerHandshakeState::Error(_) => (),
                        _ => {
                            peer.state =
                                PeerHandshakeState::Error(format!("disconnected: {}", reason))
                        }
                    },
                    _ => impossible!(),
                }
            }
            Action::Send(peer_num, action) => match self.get_peer(peer_num) {
                None => None,
                Some(peer) => {
                    let write_state = &mut peer.write_state;
                    match action {
                        SendAction::SendData(data) => write_state.start_write_data(data),
                        SendAction::ChunkSent => write_state.send_next_chunk(),
                        SendAction::ChunkError(reason) => write_state.set_error(reason),
                    }
                }
            },
            _ => unimplemented!(),
        }
    }
}

struct HandshakeMiddleware {
    port: u16,
    pub_key: PublicKey,
    pow: ProofOfWork,
    nonce: Nonce,
    chain_name: String,
    db_version: u16,
    p2p_version: u16,
}

impl HandshakeMiddleware {
    fn get_connection_message(&self) -> ConnectionMessage {
        todo!()
    }
    fn get_metadata_message(&self) -> MetadataMessage {
        todo!()
    }

    fn send_message<M: BinaryWrite>(
        &self,
        peer_num: usize,
        data: M,
        store: &Store<NodeState, Action>,
    ) -> Result<(), BinaryMessageError> {
        let encoded = data.as_bytes()?;
        store.dispatch(Action::Send(peer_num, SendAction::SendData(encoded)));
        Ok(())
    }

    fn validate_metadata(&self, metadata: MetadataMessage) -> Result<(), NackInfo> {
        todo!()
    }
}

impl Middleware<NodeState, Action> for HandshakeMiddleware {
    fn apply(&mut self, store: &mut Store<NodeState, Action>, action: Action) -> Option<Action> {
        match action {
            Action::Handshake(peer_num, action) => {
                let state = store.get_state();
                let peer = state.get_peer(peer_num).unwrap_or_else(|| &Peer::new());
                let action = match action {
                    HandshakeAction::ConnectionRecv(incoming, connection) => match peer.state {
                        PeerHandshakeState::ConnectionExchange(_, true) => {
                            Some(HandshakeAction::Blacklist("duplicate connection message"))
                        }
                        PeerHandshakeState::ConnectionExchange(_, _)
                            if state.already_connected(peer_num, peer) =>
                        {
                            Some(HandshakeAction::Disconnect("already connected"))
                        }
                        PeerHandshakeState::ConnectionExchange(_, _)
                            if state.blacklisted(peer_num, peer) =>
                        {
                            Some(HandshakeAction::Disconnect("blacklisted"))
                        }
                        PeerHandshakeState::ConnectionExchange(false, _)
                            if state.can_accept_more_connections() =>
                        {
                            Some(HandshakeAction::Disconnect("too many connections"))
                        }
                        PeerHandshakeState::ConnectionExchange(sent, _) => {
                            if sent {
                                store.dispatch(
                                    (
                                        peer_num,
                                        HandshakeAction::SendMetadata(self.get_metadata_message()),
                                    )
                                        .into(),
                                );
                            } else if incoming {
                                store.dispatch(
                                    (
                                        peer_num,
                                        HandshakeAction::SendConnection(
                                            self.get_connection_message(),
                                        ),
                                    )
                                        .into(),
                                );
                            }
                            Some(action)
                        }
                        _ => Some(HandshakeAction::Blacklist(
                            "wrong state for connection message",
                        )),
                    },
                    HandshakeAction::SendConnection(connection) => {
                        if let Err(err) = self.send_message(connection, store) {
                            Some(HandshakeAction::Error(format!("error: {}", err)))
                        } else {
                            Some(action)
                        }
                        None
                    }
                    HandshakeAction::ConnectionSent(_) => match peer.state {
                        PeerHandshakeState::ConnectionExchange(false, received) => {
                            if received {
                                store.dispatch(
                                    (
                                        peer_num,
                                        HandshakeAction::SendMetadata(self.get_metadata_message()),
                                    )
                                        .into(),
                                );
                            }
                            Some(action)
                        }
                        _ => {
                            impossible!("Should not happen");
                        }
                    },

                    HandshakeAction::MetadataRecv(metadata) => match peer.state {
                        PeerHandshakeState::MetadataExchange(_, true) => {
                            Some(HandshakeAction::Blacklist("duplicate metadata message"))
                        }
                        PeerHandshakeState::MetadataExchange(_, _) => {
                            match self.validate_metadata(metadata) {
                                Ok(_) => Some(action),
                                Err(motive) => Some(HandshakeAction::Nack(motive, true)),
                            }
                        }
                        _ => Some(HandshakeAction::Blacklist(
                            "wrong state for metadata message",
                        )),
                    },
                    HandshakeAction::SendMetadata(metadata) => {
                        if let Err(err) = self.send_message(metadata, store) {
                            Some(HandshakeAction::Error(format!("error: {}", err)))
                        } else {
                            Some(action)
                        }
                    }
                    HandshakeAction::MetadataSent(metadata) => match peer.state {
                        PeerHandshakeState::MetadataExchange(false, received) => {
                            if received {
                                store.dispatch((peer_num, HandshakeAction::SendAck).into());
                            }
                            Some(action)
                        }
                        _ => {
                            impossible!("Should not happen");
                        }
                    },

                    HandshakeAction::AckRecv => match peer.state {
                        PeerHandshakeState::AckExchange(_, true) => {
                            Some(HandshakeAction::Blacklist("duplicate ack message"))
                        }
                        PeerHandshakeState::AckExchange(sent, _) => {
                            if sent {
                                store.dispatch((peer_num, HandshakeAction::Complete).into());
                            }
                            Some(action)
                        }
                        _ => Some(HandshakeAction::Blacklist("wrong state for ack message")),
                    },
                    HandshakeAction::SendAck => {
                        if let Err(err) = self.send_message(AckMessage::Ack, store) {
                            Some(HandshakeAction::Error(format!("error: {}", err)))
                        } else {
                            Some(action)
                        }
                    }
                    HandshakeAction::AckSent => match peer.state {
                        PeerHandshakeState::AckExchange(false, received) => {
                            if received {
                                store.dispatch((peer_num, HandshakeAction::Complete).into());
                            }
                            Some(action)
                        }
                        _ => {
                            impossible!("Should not happen");
                        }
                    },

                    HandshakeAction::NackRecv(nack_info) => match peer.state {
                        PeerHandshakeState::AckExchange(_, true) => {
                            Some(HandshakeAction::Blacklist("duplicate nack message"))
                        } // duplicate Nack
                        PeerHandshakeState::ConnectionExchange(_, _)
                        | PeerHandshakeState::MetadataExchange(_, _) => {
                            Some(HandshakeAction::Blacklist("received nack message"))
                        } // Nack while handhaking TODO analyse nack_motive
                        _ => Some(HandshakeAction::Blacklist("wrong state for nack message")), // Nack after handshake
                    },
                    HandshakeAction::SendNack(nack_info) => {
                        self.send_message(nack_info, store, HandshakeAction::NackSent)
                    }
                    HandshakeAction::NackSent(nack_info) => match peer.state {
                        PeerHandshakeState::AckExchange(false, received) => {
                            if received {
                                store.dispatch(
                                    (peer_num, HandshakeAction::Blacklist("peer sent nack")).into(),
                                );
                            }
                            Some(action)
                        }
                        _ => {
                            impossible!(None; "Should not happen");
                        }
                    },

                    HandshakeAction::Disconnect(_)
                    | HandshakeAction::Blacklist(_)
                    | HandshakeAction::Nack(_, _) => Some(action),
                };
                action.map(|handshake_action| (peer_num, handshake_action).into())
            }
            _ => unimplemented!(),
        }
    }
}

struct MessageSendMiddleware {}

impl MessageSendMiddleware {

}

pub const CONTENT_LENGTH_MAX: usize =
    tezos_messages::p2p::binary_message::CONTENT_LENGTH_MAX - crypto::crypto_box::BOX_ZERO_BYTES;


impl Middleware<NodeState, Action> for MessageSendMiddleware {
    fn apply(&mut self, store: &mut Store<NodeState, Action>, action: Action) -> Option<Action> {
        match action {
            Action::Send(peer_num, action) => {
                let peer = if let Some(peer) = store.get_state().get_peer(peer_num) {
                    peer
                } else {
                    impossible!("Send action for non-existing peer");
                    return None;
                };
                let action = match action {
                    SendAction::SendData(binary_message) => match peer.message_send_state {
                        SendState::Idle => {
                            store.dispatch((peer_num, SendAction::SendNextChunk).into());
                            Some(action)
                        }
                        SendState::Sending { .. } => impossible!(None; "Cannot send another message while in sending state"),
                    }
                    SendAction::SendUnencrypted => match peer.message_send_state {
                        SendState::Idle => impossible!(None; "Cannot send next chunk while in idle state"),
                        SendState::Sending { binary_message, .. } if binary_message.len() > tezos_messages::p2p::binary_message::CONTENT_LENGTH_MAX => {
                            Some(SendAction::Error(format!("Message is to big to be sent unencrypted")))
                        }
                        SendState::Sending { next_chunk_start_pos, .. } if next_chunk_start_pos != 0 => {
                            Some(SendAction::Error(format!("Trying to sent message unencrypted not from beginning")))
                        }
                        SendState::Sending { binary_message, .. } => {
                            store.dispatch((peer_num, NetworkAction::NetSendChunk(binary_message.to_vec())).into());
                            Some(action)
                        }
                    }
                    SendAction::SendEncrypted => match peer.message_send_state {
                        SendState::Idle => impossible!(None; "Cannot send next chunk while in idle state"),
                        SendState::Sending { binary_message, next_chunk_start_pos,  } if next_chunk_start_pos != 0 => {
                            Some(SendAction::Error(format!("Trying to sent message encrypted not from beginning")))
                        }
                        send_state @ SendState::Sending { .. } => {
                            store.dispatch((peer_num, NetworkAction::NetSendChunk(send_state.get_next_chunk(CONTENT_LENGTH_MAX).to_vec())).into());
                            Some(action)
                        }
                    }
                    SendAction::SendNextChunk => match peer.message_send_state {
                        SendState::Idle => impossible!(None; "Cannot send next chunk while in idle state"),
                        SendState::Sending { binary_message, next_chunk_start_pos } => {
                            let chunk = peer.message_send_state.get_next_chunk(chunk_max_size)
                            Some(action)
                        }
                    }
                };
                action.map(|send_action| (peer_num, send_action).into())
            }
            Action::Network(peer_num, action) => {
                match action {
                    NetworkAction::ChunkSent(bytes)
                }
            }
            _ => Some(action),
        }
    }
}

enum ChunksAction {
    SizeReady,
    ChunkReady,
    MessageReady,

    SendChunk,
    ChunkSent,
    MessageSent,
}

pub fn message_write_middleware(
    store: &mut Store<NodeState, Action>,
    action: Action,
) -> Option<Action> {
    match action {
        Action::Send(action) => {
            let state = store.get_state().get_send_state();
            match action {
                SendAction::Message(msg) => match state {
                    SendState::Idle => Some(action),
                },
            }
        }
    }
}

enum ChunksState {
    /// Not expecting any data
    Idle,
    /// Chunk size is pending
    SizePending,
}

enum NetworkAction {
    NetConnectTo(SocketAddr),
    SendChunk(Vec<u8>),

    Tick,

    NetConnected(TcpStream),
    ChunkSent(usize),
}

struct NetworkMiddleware {
    poll: Poll,
    listener: Option<TcpListener>,
    streams: HashMap<usize, ChunkedStream>,
    next_token: usize,
}

const SERVER_TOKEN: usize = 0;

impl NetworkMiddleware {
    fn try_new(poll: Poll) -> io::Result<Self> {
        let poll = Poll::new()?;
        Ok(Self {
            poll: Poll::new()?,
            listener: None,
            streams: HashMap::new(),
            next_token: SERVER_TOKEN + 1,
        })
    }

    fn listen(
        &mut self,
        store: &mut Store<NodeState, Action>,
        socket_addr: SocketAddr,
    ) -> io::Result<()> {
        if self.listener.is_some() {
            let err = io::Error::new(io::ErrorKind::AlreadyExists, "Already listening");
            store.dispatch(err.into());
            return Ok(());
        }
        let listener = TcpListener::bind(socket_addr)?;
        let token = Token(SERVER_TOKEN);
        self.poll
            .registry()
            .register(listener, token, Interest::READABLE)?;
        self.listener = Some(listener);
        Ok(())
    }

    fn connect_to(
        &mut self,
        store: &mut Store<NodeState, Action>,
        socket_addr: SocketAddr,
    ) -> io::Result<()> {
        let mut stream = TcpStream::connect(socket_addr)?;
        self.connected(store, stream)?;
        Ok(())
    }

    fn connected(
        &mut self,
        store: &mut Store<NodeState, Action>,
        mut stream: TcpStream,
    ) -> io::Result<()> {
        let token = Token(self.next_token);
        self.next_token += 1;
        self.poll.registry().register(
            &mut stream,
            token,
            Interest::READABLE.and(Interest::WRITABLE),
        )?;
        self.streams.insert(token, ChunkedStream::new(stream));
        Ok(())
    }

    fn write_data(&mut self, peer_num: usize, store: &mut Store<NodeState, Action>) {
        if let Some(stream) = self.streams.get_mut(&peer_num) {
            loop {
                match stream.write_data() {
                    Ok(None) => (),
                    Ok(Some(bytes)) => store.dispatch((peer_num, NetworkAction::ChunkSent(bytes)).into()),
                    Err(ref err) if would_block(err) => break,
                    Err(err) => store.dispatch((peer_num, err).into()),
                }
            }
        } else {
            store.dispatch((peer_num, NetworkAction::Error(format!("Non-exising peer write: {}", peer_num))));
        }
    }

    fn poll_streams(&mut self, store: &mut Store<NodeState, Action>) -> io::Result<()> {
        let events = Events::with_capacity(128);
        self.poll.poll(&mut events, Duration::default())?;
        for event in events.iter() {
            if event.token().0 == SERVER_TOKEN {
                loop {
                    match self.listener.accept() {
                        Ok((connection, address)) => {
                            store.dispatch(Action::NetConnected(connection))
                        }
                        Err(ref err) if would_block(err) => break,
                        Err(err) => store.dispatch(err.into()),
                    }
                }
            } else if event.is_readable() {
                let stream = self.streams.get_mut(&event.token().0);
                loop {
                    match stream.read_data() {
                        Ok(None) => (),
                        Ok(Some(chunk)) => {
                            store.dispatch(Action::NetReceived(event.token(), chunk))
                        }
                        Err(ref err) if would_block(err) => break,
                        Err(err) => store.dispatch(err.into()),
                    }
                }
            } else if event.is_writable() {
                let stream = self.streams.get_mut(&event.token().0);
                Self::write_data(stream, store);
            }
        }
        Ok(())
    }

    fn send_data(&mut self, peer_num: usize, data: &[u8]) -> io::Result<()> {
        let stream = self.streams.get_mut(&peer_num);
        stream.set_data(&data);
        Self::write_data(stream, store);
        Ok(())
    }
}

impl Middleware<NodeState, Action> for NetworkMiddleware {
    fn apply(&mut self, store: &mut Store<NodeState, Action>, action: Action) -> Option<Action> {
        match action {
            Action::Network(action) => {
                if let Err(err) = match action {
                    NetworkAction::NetConnectTo(socket_addr) => self.connect_to(store, socket_addr),
                    NetworkAction::NetConnected(stream) => self.connected(store, stream),
                    NetworkAction::NetSendChunk(peer_num, data) => self.send_data(peer_num, data),
                    NetworkAction::NetTick => self.poll_streams(store),
                    action => return Some(action),
                } {
                    if !would_block(&err) {
                        store.dispatch(err.into());
                    }
                }
                None
            }
            _ => Some(action),
        }
    }
}

struct ChunkedStream {
    stream: TcpStream,
    in_buff: Vec<u8>,
    in_size: usize,
    out_buff: Vec<u8>,
    out_size: usize,
}

impl ChunkedStream {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            in_buff: Vec::new(),
            in_size: 0,
            out_buff: Vec::new(),
            out_size: 0,
        }
    }

    fn read_data(&mut self) -> io::Result<Option<Vec<u8>>> {
        if self.in_size < 2 {
            let r = self.stream.read(&self.in_buff[self.in_size..2])?;
            self.in_size += r;
            if self.in_size < 2 {
                return Ok(None);
            }
            if self.in_size == 2 {
                let length = ((self.in_buff[0] as u16) << 1) + self.in_buff[1];
                self.in_buff.resize(length + 2, 0)
            }
        }
        let r = self.stream.read(&self.in_buff[self.in_size..])?;
        self.in_size += r;
        if self.in_size < self.in_buff.len() {
            Ok(Some(mem::replace(&mut self.in_buff, Vec::new(2))))
        } else {
            Ok(None)
        }
    }

    fn set_data(&mut self, data: &[u8]) {
        self.out_buff = (data.len() as u16).to_be_bytes().to_vec();
        self.out_buff.extend_from_slice(data);
    }

    fn write_data(&mut self) -> io::Result<()> {
        let r = self.stream.write(&self.out_buff[self.out_size..])?;
        self.out_size += r;
    }
}

fn would_block(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::WouldBlock
}
*/
