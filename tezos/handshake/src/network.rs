use std::{collections::HashMap, convert::TryInto, io::{self, Read, Write}, marker::PhantomData, mem, net::SocketAddr, time::Duration};

use mio::{
    event::Source,
    net::{TcpListener, TcpStream},
    Events, Interest, Poll, Token,
};

use crate::redux::{Dispatcher, Middleware, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

/// Low-level actions for an abstract server.
#[derive(Debug)]
pub enum NetworkAction {
    // request actions
    /// Starts listening on the provided address.
    Listen(SocketAddr),
    /// Initiates outgoing connection to the specified address.
    ConnectTo(SocketAddr),
    /// Reads from the specified stream.
    Read(StreamId, usize),
    /// Writes to the specified stream.
    Write(StreamId, Vec<u8>),

    // reply actions
    /// Signals that the server is listening.
    Listening,
    /// Incoming connection has been accepted.
    Accepted(StreamId, SocketAddr),
    /// Outgoing connnection has been established.
    Connected(StreamId, SocketAddr),
    /// Portion of incoming data of specified size from the stream is available.
    ReadReady(StreamId, Vec<u8>),
    /// The read channel is closed
    ReadClosed(StreamId),
    /// Portion of data of specified size is written to the stream.
    WriteReady(StreamId),
    /// The write channel is closed
    WriteClosed(StreamId),

    Tick,

    /// Error occurred.
    Error(NetworkError),
}

#[derive(Debug)]
pub enum NetworkError {
    IO(io::Error),
    AlreadyListening,
    StreamIO(StreamId, io::Error),
    StreamNotFound(StreamId),
    AlreadyReading(StreamId),
    AlreadyWriting(StreamId),
}

impl From<NetworkError> for NetworkAction {
    fn from(error: NetworkError) -> Self {
        Self::Error(error)
    }
}

impl From<io::Error> for NetworkError {
    fn from(error: io::Error) -> Self {
        Self::IO(error)
    }
}

impl From<io::Error> for NetworkAction {
    fn from(error: io::Error) -> Self {
        Self::Error(error.into())
    }
}

impl From<(StreamId, io::Error)> for NetworkError {
    fn from((stream_id, error): (StreamId, io::Error)) -> Self {
        Self::StreamIO(stream_id, error)
    }
}

impl From<(StreamId, io::Error)> for NetworkAction {
    fn from((stream_id, error): (StreamId, io::Error)) -> Self {
        Self::Error((stream_id, error).into())
    }
}

/// State capable for async reading and writing
pub trait NetworkState {
    /// Returns `true` if listening for incoming connections.
    fn is_listening(&self) -> bool;
    /// Returns `true` if the state is reading from `stream_id`.
    fn is_reading(&self, stream_id: StreamId) -> bool;
    /// Returns `true` if the state is writing to `stream_id`.
    fn is_writing(&self, stream_id: StreamId) -> bool;

    /// Start listening for incoming connections.
    fn start_listening(&mut self);

    /// Add network connection
    fn connected(&mut self, stream_id: StreamId, socket_addr: SocketAddr, incoming: bool);

    /// Starts reading from stream `stream_id`.
    fn start_read(&mut self, stream_id: StreamId, size: usize);

    /// Starts writing to the `stream_id`.
    fn start_write(&mut self, stream_id: StreamId, bytes: Vec<u8>);

    /// Updates the state with the `bytes` read from the stream `stream_id`.
    fn read_done(&mut self, stream_id: StreamId, bytes: Vec<u8>);
    /// Updates the state with the closed read part of the stream `stream_id`.
    fn read_closed(&mut self, stream_id: StreamId);
    /// Updates the state after write to `stream_id` is completed.
    fn write_done(&mut self, stream_id: StreamId);
    /// Updates the state with the closed write part of the stream `stream_id`.
    fn write_closed(&mut self, stream_id: StreamId);
    /// Updates the state with error
    fn network_error(&mut self, error: NetworkError);

    /// Performs idle update
    fn idle(&mut self);
}

pub fn reduce<S: State<A> + NetworkState, A: From<NetworkAction>>(
    state: &mut S,
    action: NetworkAction,
) {
    match action {
        NetworkAction::Listen(_) => (),
        NetworkAction::ConnectTo(_) => (),
        NetworkAction::Connected(stream_id, socket_addr) => state.connected(stream_id, socket_addr, false),
        NetworkAction::Accepted(stream_id, socket_addr) => state.connected(stream_id, socket_addr, true),
        NetworkAction::Listening => state.start_listening(),
        NetworkAction::Read(stream_id, size) => state.start_read(stream_id, size),
        NetworkAction::Write(stream_id, bytes) => state.start_write(stream_id, bytes),
        NetworkAction::ReadReady(stream_id, bytes) => state.read_done(stream_id, bytes),
        NetworkAction::ReadClosed(stream_id) => state.read_closed(stream_id),
        NetworkAction::WriteReady(stream_id) => state.write_done(stream_id),
        NetworkAction::WriteClosed(stream_id) => state.write_closed(stream_id),
        NetworkAction::Tick => state.idle(),
        NetworkAction::Error(err) => state.network_error(err),
    }
}

pub struct NetworkMiddleware<S, A> {
    poll: Poll,
    listener: Option<(usize, TcpListener)>,
    streams: HashMap<StreamId, StreamBuffer>,
    next_token: usize,
    phantom: PhantomData<(S, A)>,
}

impl<S: State<A> + NetworkState, A: From<NetworkAction>> NetworkMiddleware<S, A> {
    pub fn try_new() -> io::Result<Self> {
        Ok(Self {
            poll: Poll::new()?,
            listener: None,
            streams: HashMap::new(),
            next_token: 0,
            phantom: PhantomData,
        })
    }

    fn listen(
        &mut self,
        socket_addr: SocketAddr,
        mut dispatch: impl FnMut(NetworkAction),
    ) -> Result<(), NetworkError> {
        if self.listener.is_some() {
            Err(NetworkError::AlreadyListening)
        } else {
            match TcpListener::bind(socket_addr) {
                Ok(mut listener) => match self.register(&mut listener, Interest::READABLE) {
                    Ok(listener_num) => {
                        self.listener = Some((listener_num, listener));
                        dispatch(NetworkAction::Listening);
                        Ok(())
                    }
                    Err(err) => Err(err),
                },
                Err(err) => Err(err.into()),
            }
        }
    }

    fn register<T: Source>(
        &mut self,
        some: &mut T,
        interest: Interest,
    ) -> Result<usize, NetworkError> {
        let num = self.next_token;
        self.poll.registry().register(some, Token(num), interest)?;
        self.next_token += 1;
        Ok(num)
    }

    fn connect_to(
        &mut self,
        socket_addr: SocketAddr,
        mut dispatch: impl FnMut(NetworkAction),
    ) -> Result<(), NetworkError> {
        match TcpStream::connect(socket_addr) {
            Ok(mut stream) => {
                match self.register(&mut stream, Interest::READABLE.add(Interest::WRITABLE)) {
                    Ok(stream_num) => {
                        let stream_id = StreamId(stream_num);
                        self.streams.insert(stream_id, StreamBuffer::new(stream));
                        dispatch(NetworkAction::Connected(stream_id, socket_addr));
                        Ok(())
                    }
                    Err(err) => Err(err),
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    fn accept_incoming(&mut self, dispatch: &mut impl FnMut(NetworkAction)) {
        self.listener
            .as_mut()
            .map(|(_, listener)| {
                let mut res = Vec::new();
                loop {
                    match listener.accept() {
                        Err(ref err) if would_block(err) => break,
                        r => res.push(r),
                    }
                }
                res
            })
            .map(|res| {
                res.into_iter().for_each(|res| match res {
                    Ok((mut stream, address)) => match self
                        .register(&mut stream, Interest::READABLE.add(Interest::WRITABLE))
                    {
                        Ok(stream_num) => {
                            let stream_id = StreamId(stream_num);
                            self.streams.insert(stream_id, StreamBuffer::new(stream));
                            dispatch(NetworkAction::Accepted(stream_id, address));
                        }
                        Err(err) => dispatch(err.into()),
                    },
                    Err(err) => dispatch(err.into()),
                })
            });
    }

    fn poll_streams(&mut self, state: &S, dispatch: &mut impl FnMut(NetworkAction)) -> Result<(), NetworkError> {
        let mut events = Events::with_capacity(128);
        self.poll.poll(&mut events, Some(Duration::from_millis(0)))?;
        let listener_num = self.listener.as_ref().map(|(num, _)| *num);
        for event in events.iter() {
            println!("event: {:?}", event);
            let stream_id = StreamId(event.token().0);
            if listener_num.map(|n| n == event.token().0).unwrap_or(false) {
                self.accept_incoming(dispatch);
            } else {
                if event.is_readable() {
                    self.read(stream_id, state, dispatch);
                } else if event.is_read_closed() {
                    dispatch(NetworkAction::ReadClosed(stream_id));
                }
                if event.is_writable() {
                    self.write(stream_id, state, dispatch);
                } else if event.is_write_closed() {
                    dispatch(NetworkAction::WriteClosed(stream_id));
               }
            }
        }
        Ok(())
    }

    fn read_stream(stream_id: StreamId, stream: &mut StreamBuffer, dispatch: &mut impl FnMut(NetworkAction)) {
        match stream.read() {
            Ok(bytes) if bytes.len() != 0 => dispatch(NetworkAction::ReadReady(stream_id, bytes)),
            Ok(_) => dispatch(NetworkAction::ReadClosed(stream_id)),
            Err(ref err) if would_block(err) => (),
            Err(err) => dispatch((stream_id, err).into()),
        }
    }

    fn write_stream(stream_id: StreamId, stream: &mut StreamBuffer, dispatch: &mut impl FnMut(NetworkAction)) {
        match stream.write() {
            Ok(_) => dispatch(NetworkAction::WriteReady(stream_id)),
            Err(ref err) if would_block(err) => (),
            Err(err) => dispatch((stream_id, err).into()),
        }
    }

    fn read(&mut self, stream_id: StreamId, _state: &S, dispatch: &mut impl FnMut(NetworkAction)) {
        println!("read");
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            Self::read_stream(stream_id, stream, dispatch);
        } else {
            dispatch(NetworkError::StreamNotFound(stream_id).into());
        }
    }

    fn write(&mut self, stream_id: StreamId, _state: &S, dispatch: &mut impl FnMut(NetworkAction)) {
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            Self::write_stream(stream_id, stream, dispatch);
        } else {
            dispatch(NetworkError::StreamNotFound(stream_id).into());
        }
    }

    fn start_read(
        &mut self,
        stream_id: StreamId,
        size: usize,
        state: &S,
        dispatch: &mut impl FnMut(NetworkAction),
    ) -> Result<(), NetworkError> {
        if state.is_reading(stream_id) {
            Err(NetworkError::AlreadyReading(stream_id))
        } else if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.set_read_size(size);
            Self::read_stream(stream_id, stream, dispatch);
            Ok(())
        } else {
            Err(NetworkError::StreamNotFound(stream_id))
        }
    }

    fn start_write(
        &mut self,
        stream_id: StreamId,
        bytes: Vec<u8>,
        state: &S,
        dispatch: &mut impl FnMut(NetworkAction),
    ) -> Result<(), NetworkError> {
        if state.is_writing(stream_id) {
            Err(NetworkError::AlreadyWriting(stream_id))
        } else if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.set_write_bytes(bytes);
            Self::write_stream(stream_id, stream, dispatch);
            Ok(())
        } else {
            Err(NetworkError::StreamNotFound(stream_id))
        }
    }

    fn apply_network(
        &mut self,
        state: &S,
        action: NetworkAction,
        dispatcher: &mut Dispatcher<A>,
    ) -> Option<NetworkAction> {
        let mut dispatch = Self::dispatch(dispatcher);
        let result = match &action {
            NetworkAction::Listen(socket_addr) => self.listen(*socket_addr, dispatch),
            NetworkAction::ConnectTo(socket_addr) => self.connect_to(*socket_addr, dispatch),
            NetworkAction::Read(stream_id, size) => self.start_read(*stream_id, *size, state, &mut dispatch),
            NetworkAction::Write(stream_id, bytes) => self.start_write(*stream_id, bytes.clone(), state, &mut dispatch),
            NetworkAction::Tick => self.poll_streams(state, &mut dispatch),
            _ => Ok(()),
        };
        match result {
            Ok(_) => Some(action),
            Err(err) => Some(err.into()),
        }
    }

    fn dispatch<'a>(dispatcher: &'a mut Dispatcher<A>) -> impl FnMut(NetworkAction) + 'a {
        move |action| dispatcher.push_front(action.into())
    }
}

impl<S, A> Middleware<S, A> for NetworkMiddleware<S, A>
where
    S: State<A> + NetworkState,
    A: From<NetworkAction> + TryInto<NetworkAction, Error = A>,
{
    fn apply(&mut self, state: &S, action: A, dispatcher: &mut Dispatcher<A>) -> Option<A> {
        match action.try_into() {
            Ok(net_action) => self
                .apply_network(state, net_action, dispatcher)
                .map(Into::into),
            Err(action) => Some(action),
        }
    }
}

struct StreamBuffer {
    stream: TcpStream,
    read_buff: Vec<u8>,
    read_pos: usize,
    write_buff: Vec<u8>,
    write_pos: usize,
}

impl StreamBuffer {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            read_buff: Vec::new(),
            read_pos: 0,
            write_buff: Vec::new(),
            write_pos: 0,
        }
    }

    fn set_read_size(&mut self, size: usize) {
        self.read_buff.resize(size, 0);
        self.read_pos = 0;
    }

    fn set_write_bytes(&mut self, bytes: Vec<u8>) {
        self.write_buff = bytes;
        self.write_pos = 0;
    }

    fn read(&mut self) -> io::Result<Vec<u8>> {
        let buff = &mut self.read_buff[self.read_pos..];
        let mut bytes_read = 0;
        while bytes_read < buff.len() {
            match self.stream.read(&mut buff[bytes_read..]) {
                Ok(bytes) => {
                    bytes_read += bytes;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(mem::replace(&mut self.read_buff, Vec::new()))
    }

    fn write(&mut self) -> io::Result<()> {
        let buff = &self.write_buff[self.write_pos..];
        let mut bytes_wrote = 0;
        while bytes_wrote < buff.len() {
            match self.stream.write(&buff[bytes_wrote..]) {
                Ok(bytes) => {
                    bytes_wrote += bytes;
                }
                Err(e) => return Err(e),
            }
        }
        self.write_buff = Vec::new();
        Ok(())
    }
}

fn would_block(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::WouldBlock
}

#[cfg(test)]
mod tests {}
