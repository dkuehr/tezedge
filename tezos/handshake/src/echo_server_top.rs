use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
};

use crate::{
    network::StreamId,
    redux::{Dispatcher, State, Store},
};

#[derive(Debug)]
enum EchoServerState {
    Initial,
    Active {
        clients: HashMap<StreamId, StreamState>,
    },
    Error(String),
}

/// Each client can be in one of those states:
///
///              /--> Disconnected
/// Initial -> Listening <-> Echoing
///   \----------\-> Error <----/
#[derive(Debug)]
enum StreamState {
    Initial,
    Listening,
    Echoing,
    Disconnected,
    Error,
}

enum EchoServerAction {
    Start,
    Started,
    Connected { stream_id: StreamId },
    Listen { stream_id: StreamId },
    Shout { stream_id: StreamId, data: Vec<u8> },
    Echo { stream_id: StreamId, data: Vec<u8> },
    EchoSent { stream_id: StreamId },
    Disconnect { stream_id: StreamId },
    Error { err: EchoServerError },
}

enum EchoServerError {
    /// Server cannot be started.
    CannotStartServer,
    /// Line is not fully read when the client disconnected.
    IncompleteLine { stream_id: StreamId },
    /// Client closed connection while writing echo reply.
    CannotEcho { stream_id: StreamId },
}

impl EchoServerState {
    fn with_stream<T, F: FnOnce(&StreamState) -> T>(&self, stream_id: StreamId, f: F) -> Option<T> {
        if let Self::Active { clients } = self {
            clients.get(&stream_id).map(f)
        } else {
            None
        }
    }

    fn with_stream_mut<T, F: FnOnce(&mut StreamState) -> T>(
        &mut self,
        stream_id: StreamId,
        f: F,
    ) -> Option<T> {
        if let Self::Active { clients } = self {
            clients.get_mut(&stream_id).map(f)
        } else {
            None
        }
    }
}

impl State<EchoServerAction> for EchoServerState {
    fn reduce(&mut self, action: EchoServerAction) {
        match action {
            EchoServerAction::Start => match self {
                EchoServerState::Initial => {
                    *self = Self::Active {
                        clients: HashMap::new(),
                    };
                }
                _ => (),
            },
            EchoServerAction::Started => (),
            EchoServerAction::Connected { stream_id } => match self {
                EchoServerState::Active { clients } => {
                    clients.insert(stream_id, StreamState::Initial);
                }
                _ => (),
            },
            EchoServerAction::Listen { stream_id } => {
                self.with_stream_mut(stream_id, |stream| match stream {
                    StreamState::Initial => *stream = StreamState::Listening,
                    _ => (),
                });
            }
            EchoServerAction::Shout { stream_id, .. } => {
                self.with_stream_mut(stream_id, |stream| match stream {
                    StreamState::Listening => *stream = StreamState::Echoing,
                    _ => (),
                });
            }
            EchoServerAction::Echo { .. } => (),
            EchoServerAction::EchoSent { stream_id } => {
                self.with_stream_mut(stream_id, |stream| match stream {
                    StreamState::Echoing => *stream = StreamState::Listening,
                    _ => (),
                });
            }
            EchoServerAction::Disconnect { stream_id } => {
                self.with_stream_mut(stream_id, |stream| match stream {
                    StreamState::Error | StreamState::Disconnected => (),
                    _ => *stream = StreamState::Disconnected,
                });
            }
            EchoServerAction::Error { err } => match err {
                EchoServerError::CannotStartServer => match self {
                    EchoServerState::Error(_err) => (),
                    _ => {
                        *self = Self::Error("error".to_string());
                    }
                },
                EchoServerError::IncompleteLine { stream_id } => {
                    self.with_stream_mut(stream_id, |stream| {
                        match stream {
                            StreamState::Error | StreamState::Disconnected => (),
                            _ => *stream = StreamState::Error,
                        };
                    });
                }
                EchoServerError::CannotEcho { stream_id } => {
                    self.with_stream_mut(stream_id, |stream| {
                        match stream {
                            StreamState::Error | StreamState::Disconnected => (),
                            _ => *stream = StreamState::Error,
                        };
                    });
                }
            },
        }
    }
}

fn echo_server_middleware(
    state: &EchoServerState,
    action: EchoServerAction,
    dispatcher: &mut Dispatcher<EchoServerAction>,
) -> Option<EchoServerAction> {
    match &action {
        EchoServerAction::Start { .. } => match state {
            EchoServerState::Initial => dispatcher.dispatch(EchoServerAction::Started),
            _ => (),
        },
        EchoServerAction::Shout { stream_id, data } => {
            state.with_stream(*stream_id, |stream| match stream {
                StreamState::Listening => dispatcher.dispatch(EchoServerAction::Echo {
                    stream_id: *stream_id,
                    data: data.clone(),
                }),
                _ => (),
            });
        }
        EchoServerAction::Echo { stream_id, .. } => {
            state.with_stream(*stream_id, |stream| match stream {
                StreamState::Echoing => dispatcher.dispatch(EchoServerAction::EchoSent {
                    stream_id: *stream_id,
                }),
                _ => (),
            });
        }
        _ => (),
    };
    Some(action)
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_connection() {
        let started = Rc::new(RefCell::new(0));
        let echoed = Rc::new(RefCell::new(0));
        let started1 = started.clone();
        let echoed1 = echoed.clone();
        let middleware = move |state: &EchoServerState,
        action: EchoServerAction,
        _: &mut Dispatcher<EchoServerAction>| {
            match &action {
                EchoServerAction::Started => {
                    assert!(matches!(state, &EchoServerState::Active { .. }));
                    *started1.borrow_mut() += 1;
                }
                EchoServerAction::Echo { stream_id, data } => {
                    assert_eq!((*stream_id, data), (StreamId(1), &vec![0x00]));
                    *echoed1.borrow_mut() += 1;
                }
                _ => (),
            };
            Some(action)
        };

        let mut store = Store::new(EchoServerState::Initial);
        store.add_middleware(Box::new(echo_server_middleware));
        store.add_middleware(Box::new(middleware));

        store.dispatch_iter([
            EchoServerAction::Start,
            EchoServerAction::Connected {
                stream_id: StreamId(1),
            },
            EchoServerAction::Listen {
                stream_id: StreamId(1),
            },
            EchoServerAction::Shout {
                stream_id: StreamId(1),
                data: vec![0x00],
            },
        ]);
        store.event_loop();

        assert_eq!(*started.borrow(), 1, "Should be started exactly once");
        assert_eq!(*echoed.borrow(), 1, "Should echo exactly once");
    }

}
