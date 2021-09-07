use std::{collections::HashMap, convert::TryFrom, time::Duration};

use handshake::{network::{self, NetworkAction, NetworkMiddleware, NetworkState, StreamId}, redux::{Dispatcher, State, Store}};

#[derive(Debug)]
enum EchoServerState {
    Initial,
    Listening {
        streams: HashMap<StreamId, StreamState>,
    },
    Error {
        error: String,
    },
}

#[derive(Debug)]
enum StreamState {
    Idle,
    Reading { data: Vec<u8>, reading: bool },
    Ready { data: Vec<u8> },
    Writing { writing: bool },
}

#[derive(Debug)]
enum EchoServerAction {
    Start,
    SendIfReady(StreamId),

    Net(NetworkAction),
}

impl StreamState {
    fn is_reading(&self) -> bool {
        if let Self::Reading { reading, .. } = self {
            *reading
        } else {
            false
        }
    }

    fn is_writing(&self) -> bool {
        if let Self::Writing { writing } = self {
            *writing
        } else {
            false
        }
    }

    fn start_read(&mut self) {
        match self {
            Self::Reading { reading, .. } => *reading = true,
            _ => {
                *self = Self::Reading {
                    data: Vec::new(),
                    reading: true,
                }
            }
        }
    }

    fn append_bytes(&mut self, bytes: Vec<u8>) {
        match self {
            Self::Reading { data, reading } if *reading && bytes.as_slice() != EOL => {
                data.extend(bytes);
                *reading = false;
            }
            Self::Reading { data, reading } if *reading => {
                data.extend(bytes);
                *self = Self::Ready { data: data.clone() };
            }
            _ => (),
        }
    }

    fn start_write(&mut self) {
        match self {
            Self::Writing { writing, .. } => *writing = true,
            _ => *self = Self::Writing { writing: true },
        }
    }

    fn done_write(&mut self) {
        if let Self::Writing { writing } = self {
            *writing = false;
        }
    }
}

impl EchoServerState {
    fn with_stream<T, F: FnOnce(&StreamState) -> T>(&self, stream_id: StreamId, f: F) -> Option<T> {
        if let Self::Listening { streams } = self {
            streams.get(&stream_id).map(f)
        } else {
            None
        }
    }

    fn with_mut_stream<T, F: FnOnce(&mut StreamState) -> T>(
        &mut self,
        stream_id: StreamId,
        f: F,
    ) -> Option<T> {
        if let Self::Listening { streams } = self {
            streams.get_mut(&stream_id).map(f)
        } else {
            None
        }
    }

    // actions

    fn start_server(&self, dispatcher: &mut Dispatcher<EchoServerAction>) {
        dispatcher.dispatch(NetworkAction::Listen("0.0.0.0:3300".parse().unwrap()));
    }

    fn start_receiving(&self, stream_id: StreamId, dispatcher: &mut Dispatcher<EchoServerAction>) {
        dispatcher.dispatch(NetworkAction::Read(stream_id, 1));
    }

    fn send_if_ready(&self, stream_id: StreamId, dispatcher: &mut Dispatcher<EchoServerAction>) {
        self.with_stream(stream_id, |stream| match stream {
            StreamState::Reading { reading: false, .. } => {
                dispatcher.dispatch(NetworkAction::Read(stream_id, 1))
            }
            StreamState::Ready { data } => {
                dispatcher.dispatch(NetworkAction::Write(stream_id, data.clone()))
            }
            _ => {
                panic!();
            }
        });
    }
}

impl State<EchoServerAction> for EchoServerState {
    fn reduce(&mut self, action: EchoServerAction) {
        match action {
            EchoServerAction::Net(action) => network::reduce(self, action),
            _ => (),
        }
    }
}

const EOL: &[u8] = &[0x0a];

impl NetworkState for EchoServerState {
    fn is_listening(&self) -> bool {
        matches!(self, Self::Listening { .. })
    }

    fn is_reading(&self, stream_id: StreamId) -> bool {
        self.with_stream(stream_id, StreamState::is_reading)
            .unwrap_or(false)
    }

    fn is_writing(&self, stream_id: StreamId) -> bool {
        self.with_stream(stream_id, StreamState::is_writing)
            .unwrap_or(false)
    }

    fn start_listening(&mut self) {
        *self = Self::Listening {
            streams: HashMap::new(),
        }
    }

    fn connected(
        &mut self,
        stream_id: StreamId,
        _socket_addr: std::net::SocketAddr,
        _incoming: bool,
    ) {
        if let Self::Listening { streams, .. } = self {
            streams.insert(stream_id, StreamState::Idle);
        }
    }

    fn start_read(&mut self, stream_id: StreamId, _size: usize) {
        self.with_mut_stream(stream_id, StreamState::start_read);
    }

    fn start_write(&mut self, stream_id: StreamId, _bytes: Vec<u8>) {
        self.with_mut_stream(stream_id, StreamState::start_write);
    }

    fn read_done(&mut self, stream_id: StreamId, bytes: Vec<u8>) {
        self.with_mut_stream(stream_id, move |stream| stream.append_bytes(bytes));
    }

    fn write_done(&mut self, stream_id: StreamId) {
        self.with_mut_stream(stream_id, move |stream| stream.done_write());
    }

    fn network_error(&mut self, error: handshake::network::NetworkError) {
        *self = Self::Error {
            error: format!("error: {:?}", error),
        };
    }

    fn idle(&mut self) {}
}

fn echo_server_middleware(
    state: &EchoServerState,
    action: EchoServerAction,
    dispatcher: &mut Dispatcher<EchoServerAction>,
) -> Option<EchoServerAction> {
    match action {
        EchoServerAction::Start => {
            state.start_server(dispatcher);
            None
        }
        EchoServerAction::SendIfReady(stream_id) => {
            state.send_if_ready(stream_id, dispatcher);
            None
        }
        EchoServerAction::Net(action) => {
            match &action {
                NetworkAction::Listening => (),
                NetworkAction::Accepted(stream_id, _) => {
                    state.start_receiving(*stream_id, dispatcher)
                }
                NetworkAction::ReadReady(stream_id, _) => {
                    dispatcher.dispatch(EchoServerAction::SendIfReady(*stream_id))
                }
                NetworkAction::WriteReady(stream_id) => {
                    dispatcher.dispatch(NetworkAction::Read(*stream_id, 1))
                }
                _ => (),
            }
            Some(action.into())
        }
    }
}

impl From<NetworkAction> for EchoServerAction {
    fn from(action: NetworkAction) -> Self {
        Self::Net(action)
    }
}

impl TryFrom<EchoServerAction> for NetworkAction {
    type Error = EchoServerAction;

    fn try_from(action: EchoServerAction) -> Result<Self, EchoServerAction> {
        match action {
            EchoServerAction::Net(action) => Ok(action),
            _ => Err(action),
        }
    }
}

fn debug_middleware(
    _state: &EchoServerState,
    action: EchoServerAction,
    _dispatcher: &mut Dispatcher<EchoServerAction>,
) -> Option<EchoServerAction> {
    println!("action: {:?}", action);
    Some(action)
}

#[test]
fn simple_test() {
    let mut store = Store::new_with_idle(
        EchoServerState::Initial,
        || NetworkAction::Tick.into(),
        Duration::from_millis(1000),
    );
    store.add_middleware(Box::new(debug_middleware));
    store.add_middleware(Box::new(NetworkMiddleware::try_new().unwrap()));
    store.add_middleware(Box::new(echo_server_middleware));
    store.event_loop_with(EchoServerAction::Start);
}
