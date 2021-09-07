use std::{collections::VecDeque, thread, time::Duration};


pub struct Store<S, A> {
    state: S,
    idle_event: Option<(fn() -> A, Duration)>,
    middlewares: Vec<Box<dyn Middleware<S, A>>>,
    dispatcher: Dispatcher<A>,
}

pub struct Dispatcher<A> {
    actions: VecDeque<A>,
}

pub trait State<A> {
    fn reduce(&mut self, action: A);
}

pub trait Middleware<S, A> {
    fn apply(&mut self, state: &S, action: A, dispatcher: &mut Dispatcher<A>) -> Option<A>;
}

impl<F, S, A> Middleware<S, A> for F
where
    F: FnMut(&S, A, &mut Dispatcher<A>) -> Option<A>,
{
    fn apply(&mut self, state: &S, action: A, dispatcher: &mut Dispatcher<A>) -> Option<A> {
        self(state, action, dispatcher)
    }
}

impl<A> Dispatcher<A> {
    fn new() -> Self {
        Self {
            actions: VecDeque::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    pub fn push_front(&mut self, action: A) {
        self.actions.push_front(action)
    }

    pub fn dispatch<T: Into<A>>(&mut self, action: T) {
        self.push_front(action.into());
    }

    fn pop_back(&mut self) -> Option<A> {
        self.actions.pop_back()
    }
}

impl<S, A> Store<S, A>
where
    S: State<A> + std::fmt::Debug,
{
    pub fn new(state: S) -> Self {
        Self {
            state,
            idle_event: None,
            middlewares: Vec::new(),
            dispatcher: Dispatcher::new(),
        }
    }

    pub fn new_with_idle(state: S, idle_event: fn() -> A, pause: Duration) -> Self {
        Self {
            state,
            idle_event: Some((idle_event, pause)),
            middlewares: Vec::new(),
            dispatcher: Dispatcher::new(),
        }
    }

    pub fn add_middleware(&mut self, middleware: Box<dyn Middleware<S, A>>) {
        self.middlewares.push(middleware);
    }

    pub fn event_loop_with(&mut self, initial_action: A) {
        self.dispatcher.push_front(initial_action);
        self.event_loop()
    }

    pub fn event_loop(&mut self) {
        'main: loop {
            println!("main loop");
            while let Some(mut action) = self.dispatcher.pop_back() {
                for middleware in self.middlewares.iter_mut() {
                    if let Some(a) = middleware.apply(&self.state, action, &mut self.dispatcher) {
                        action = a;
                    } else {
                        continue 'main;
                    }
                };
                println!("pre  state: {:?}", self.state);
                self.state.reduce(action);
                println!("post state: {:?}", self.state);

                /*
                if let Some(action) = middlewares
                    .iter_mut()
                    .try_fold(action, |action, middleware| middleware.apply(state, action, dispatcher))
                {
                    state.reduce(action)
                }
                */

            }

            if let Some((event, pause)) = self.idle_event {
                loop {
                    let mut action = event();
                    for middleware in self.middlewares.iter_mut() {
                        if let Some(a) = middleware.apply(&self.state, action, &mut self.dispatcher) {
                            action = a;
                        } else {
                            continue 'main;
                        }
                    };
                    self.state.reduce(action);

                    if !self.dispatcher.is_empty() {
                        continue 'main;
                    }
                    thread::sleep(pause);
                }
            } else {
                break;
            }
        }
    }

    pub fn dispatch(&mut self, action: A) {
        self.dispatcher.push_front(action);
    }

    pub fn get_state(&self) -> &S {
        &self.state
    }
}

#[macro_export]
macro_rules! impossible {
	() => { { debug_assert!(false, "Impossible combination"); } };
    ($result:expr; $($arg:tt)*) => { { debug_assert!(false, $($arg)*); $result } };
    ($($arg:tt)*) => { { debug_assert!(false, $($arg)*); } };
}
