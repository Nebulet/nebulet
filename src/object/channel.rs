use super::dispatcher::{Dispatch, Dispatcher};
use signals::Signal;
use nabi::{Result, Error};
use object::Handle;
use alloc::{Vec, VecDeque};
use alloc::arc::Arc;
use arch::lock::Spinlock;

pub const MAX_MSGS: usize       = 1000;
pub const MAX_MSG_SIZE: usize   = 64 * 1024; // 64 KiB

pub struct Message {
    data: Vec<u8>,
    handles: Vec<Handle<Dispatcher>>,
}

impl Message {
    pub fn new(data: &[u8], handles: Vec<Handle<Dispatcher>>) -> Result<Message> {
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

    pub fn handles(&self) -> &[Handle<Dispatcher>] {
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
                Err(Error::NO_MEMORY)
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

        if let Some(peer) = peer_guard.as_ref() {
            let signal_peer = shared.msgs.len() == MAX_MSGS;

            let msg = shared.msgs.pop_front().ok_or(Error::SHOULD_WAIT)?;

            if shared.msgs.is_empty() {
                // deassert readable signal on self
                self.signal(Signal::empty(), Signal::READABLE)?;
            }

            if signal_peer {
                peer.signal(Signal::WRITABLE, Signal::empty())?;
            }

            Ok(msg)
        } else {
            Err(Error::PEER_CLOSED)
        }
    }

    pub fn first_msg_len(&self) -> Option<usize> {
        let shared = self.shared.lock();

        shared.msgs.front().map(|msg| msg.data().len())
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
}
