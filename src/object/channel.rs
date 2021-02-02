use super::dispatcher::{Dispatch, Dispatcher};
use signals::Signal;
use nabi::{Result, Error};
use object::Handle;
use alloc::vec::Vec;
use alloc::collections::vec_deque::VecDeque;
use alloc::sync::Arc;
use arch::lock::Spinlock;

pub const MAX_MSGS: usize       = 1000;
pub const MAX_MSG_SIZE: usize   = 64 * 1024; // 64 KiB

pub struct Message {
    data: Vec<u8>,
    handles: Vec<Handle<dyn Dispatcher>>,
}

impl Message {
    pub fn new(data: &[u8], handles: Vec<Handle<dyn Dispatcher>>) -> Result<Message> {
        if data.len() > MAX_MSG_SIZE {
            return Err(Error::INVALID_ARG);
        }

        Ok(Message {
            data: data.to_vec(),
            handles,
        })
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn handles(&self) -> &[Handle<dyn Dispatcher>] {
        &self.handles
    }
}

struct SharedData {
    msgs: VecDeque<Message>,
}

/// Represents a writable
/// and readable channel
/// for transferring data
/// between processes.
pub struct Channel {
    shared: Arc<Spinlock<SharedData>>,
    peer: Spinlock<Option<Dispatch<Channel>>>,
}

impl Channel {
    pub fn new_pair() -> (Dispatch<Self>, Dispatch<Self>) {
        let shared = Arc::new(Spinlock::new(SharedData {
            msgs: VecDeque::new(),
        }));

        let first = Dispatch::new(Channel {
            shared: Arc::clone(&shared),
            peer: Spinlock::new(None),
        });

        let second = Dispatch::new(Channel {
            shared: Arc::clone(&shared),
            peer: Spinlock::new(Some(first.copy_ref())),
        });

        *first.peer.lock() = Some(second.copy_ref());

        (first, second)
    }

    pub fn peer(&self) -> Option<Dispatch<Channel>> {
        let peer_guard = self.peer.lock();
        peer_guard.as_ref().map(|dispatcher| dispatcher.copy_ref())
    }

    pub fn send(self: &Dispatch<Self>, msg: Message) -> Result<()> {
        let mut shared = self.shared.lock();

        let peer_guard = self.peer.lock();

        if let Some(peer) = peer_guard.as_ref() {
            if shared.msgs.len() == MAX_MSGS {
                Err(Error::SHOULD_WAIT)
            } else {
                shared.msgs.push_back(msg);

                if shared.msgs.len() == MAX_MSGS {
                    self.signal(Signal::empty(), Signal::WRITABLE)?;
                }

                peer.signal(Signal::READABLE, Signal::empty())?;

                Ok(())
            }
        } else {
            Err(Error::PEER_CLOSED)
        }
    }

    pub fn recv(self: &Dispatch<Self>) -> Result<Message> {
        let mut shared = self.shared.lock();

        let peer_guard = self.peer.lock();

        let signal_peer = shared.msgs.len() == MAX_MSGS;

        if let Some(msg) = shared.msgs.pop_front() {
            if shared.msgs.is_empty() {
                // deassert readable signal on self
                self.signal(Signal::empty(), Signal::READABLE)?;
            }

            if let (true, Some(peer)) = (signal_peer, peer_guard.as_ref()) {
                peer.signal(Signal::WRITABLE, Signal::empty())?;
            }

            Ok(msg)
        } else {
            if peer_guard.is_some() {
                Err(Error::SHOULD_WAIT)
            } else {
                Err(Error::PEER_CLOSED)
            }
        }
    }

    pub fn first_msg_len(&self) -> Result<usize> {
        let shared = self.shared.lock();

        shared.msgs
            .front()
            .map(|msg| msg.data().len())
            .ok_or_else(|| {
                if self.peer().is_some() {
                    Error::SHOULD_WAIT
                } else {
                    Error::PEER_CLOSED
                }
            })
    }
}

impl Dispatcher for Channel {
    fn allowed_user_signals(&self) -> Signal {
        Signal::READABLE
        | Signal::WRITABLE 
        | Signal::PEER_CLOSED
        | Signal::PEER_SIGNALED
    }

    fn allows_observers(&self) -> bool { true }

    fn on_zero_handles(&self) {
        if let Some(peer) = self.peer() {
            let mut this_guard = peer.peer.lock();
            *this_guard = None;
        }
    }
}
