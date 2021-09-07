use std::{collections::HashMap, convert::TryFrom, time::Duration};

use handshake::{network::{self, NetworkAction, NetworkMiddleware, NetworkState, StreamId}, redux::{Dispatcher, Middleware, State, Store}};

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
    Reading { data: Vec<u8> },
    Ready { data: Vec<u8> },
    Writing,

    Error(EchoError),
}

#[derive(Debug)]
enum EchoServerAction {
    Start,
    Listen(StreamId),
    Cry(StreamId, Vec<u8>),
    Echo(StreamId, Vec<u8>),

    Error(EchoError),

    Net(NetworkAction),
}

#[derive(Debug)]
enum EchoError {
    IncompleteInput,
    CannotWrite,
}

impl StreamState {
    fn is_reading(&self) -> bool {
        if let Self::Reading { .. } = self {
            true
        } else {
            false
        }
    }

    fn is_writing(&self) -> bool {
        if let Self::Writing = self {
            true
        } else {
            false
        }
    }

    fn start_read(&mut self) {
        match self {
            Self::Reading { .. } => (),
            _ => {
                *self = Self::Reading {
                    data: Vec::new(),
                }
            }
        }
    }

    fn append_bytes(&mut self, bytes: Vec<u8>) {
        match self {
            Self::Reading { data } if bytes.as_slice() != EOL => {
                data.extend(bytes);
            }
            Self::Reading { data } => {
                data.extend(bytes);
                *self = Self::Ready { data: data.clone() };
            }
            _ => (),
        }
    }

    fn start_write(&mut self) {
        match self {
            Self::Writing => (),
            _ => *self = Self::Writing,
        }
    }

    fn done_write(&mut self) {
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

    fn handle_read_ready(&self, stream_id: StreamId, bytes: &Vec<u8>, dispatcher: &mut Dispatcher<EchoServerAction>) {
        self.with_stream(stream_id, |stream| {
            match stream {
                StreamState::Reading { data } => if bytes.as_slice() == EOL {
                    dispatcher.dispatch(EchoServerAction::Echo(stream_id))
                } else {
                    dispatcher.dispatch(NetworkAction::Read(stream_id, 1))
                }
                _ => ()
            }
        })
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

    fn read_closed(&mut self, _: StreamId) {
        self.with_mut_stream(stream_id, |stream| {
            match stream {
//                StreamState::Reading { data } if !data.is_empty() => *stream &mut StreamState::Error(StreamError::IncompleteInput),
            }
        })
    }
}



fn echo_server_to_network_middleware(state: &EchoServerState, action: EchoServerAction, dispatcher: &mut Dispatcher<EchoServerAction>) -> Option<EchoServerAction> {
    match &action {
        EchoServerAction::Start => dispatcher.dispatch(NetworkAction::Listen(todo!())),
        EchoServerAction::Echo(stream_id, data) => dispatcher.dispatch(NetworkAction::Write(*stream_id, data.clone())),
        EchoServerAction::Error(err) => eprintln!("error: {:?}", err),
        _ => (),
    }
    Some(action)
}

fn network_to_echo_server_middleware(state: &EchoServerState, action: EchoServerAction, dispatcher: &mut Dispatcher<EchoServerAction>) -> Option<EchoServerAction> {
    if let EchoServerAction::Net(action) = &action {
        match action {
            NetworkAction::Listening => dispatcher.dispatch(EchoServerAction::Listening),
            NetworkAction::Accepted(stream_id, _) => dispatcher.dispatch(EchoServerAction::IncomingConnection(stream_id)),
            NetworkAction::ReadReady(stream_id, bytes) if bytes.as_slice() == EOL => state.with_stream(*stream_id, |stream| {
                if let StreamState::Reading { data } = stream {
                    let data = data.clone();
                    data.extend(data);
                    dispatcher.dispatch(EchoServerAction::Cry(*stream_id, data));
                }
            }),
            NetworkAction::ReadClosed(_) => state.with_stream(*stream_id, |stream| {
                match stream {
                    StreamState::Reading { data } if !data.is_empty() => dispatcher.dispatch(EchoServerAction::Error(EchoError::IncompleteInput(*stream_id))),
                    _ => (),
                }
            }),
            NetworkAction::WriteReady(_) => todo!(),
            NetworkAction::WriteClosed(_) => todo!(),
            NetworkAction::Tick => todo!(),
            NetworkAction::Error(_) => todo!(),
        }
    }
    Some(action)
}


struct EchoServerMiddleware {

}

impl EchoServerMiddleware {
    fn on_cry(&self, state: &EchoServerState, stream_id: StreamId, cry: &Vec<u8>, dispatcher: &mut Dispatcher<EchoServerAction>) -> Option<EchoServerAction> {
        state.with_stream(stream_id, |stream| {
            match stream {
                StreamState::Error(_) => (),
                _ => dispatcher.dispatch(EchoServerAction::Echo(stream_id, cry.clone())),
            }
        })
    }
}

impl Middleware<EchoServerState, EchoServerAction> for EchoServerMiddleware {
    fn apply(&mut self, state: &EchoServerState, action: EchoServerAction, dispatcher: &mut Dispatcher<EchoServerAction>) -> Option<EchoServerAction> {
        match &action {
            EchoServerAction::Cry(stream_id, bytes) => self.on_cry(state, stream_id, cry, dispatcher),
            _ => (),
        }
        Some(action)
    }
}

struct EchoServerToNetworkMiddleware {}

impl Middleware<EchoServerState, EchoServerAction> for EchoServerToNetworkMiddleware {
    fn apply(&mut self, state: &EchoServerState, action: EchoServerAction, dispatcher: &mut Dispatcher<EchoServerAction>) -> Option<EchoServerAction> {
        match &action {
            EchoServerAction::Start => self.on_start(state, dispatcher),
            EchoServerAction::Echo(stream_id, data) => self.on_echo(stream_id, data),
            EchoServerAction::Error(err) => self.on_error(err),
            EchoServerAction::Net(_) => (),
        }
        Some(action)
    }
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
                NetworkAction::ReadReady(stream_id, bytes) => {
                    state.handle_read_ready(*stream_id, bytes, dispatcher);
                }
                NetworkAction::ReadClosed(stream_id) => {
                    state.handle_read_closed(*stream_id, dispatcher);
                }
                NetworkAction::WriteReady(stream_id) => {
                    state.handle_write_ready(*stream_id, dispatcher);
                }
                NetworkAction::WriteClosed(stream_id) => {
                    state.handle_write_closed(*stream_id, dispatcher);
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
