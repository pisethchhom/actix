use std::cell::Cell;
use futures::unsync::oneshot::{channel, Receiver};
use futures::sync::oneshot::{channel as sync_channel, Receiver as SyncReceiver};

use actor::{Actor, Handler, ResponseType};
use context::{Context, ContextProtocol};
use envelope::Envelope;
use message::Request;
use queue::{sync, unsync};


#[doc(hidden)]
pub trait ActorAddress<A, T> where A: Actor {
    fn get(ctx: &mut Context<A>) -> T;
}

impl<A> ActorAddress<A, Address<A>> for A where A: Actor {
    fn get(ctx: &mut Context<A>) -> Address<A> {
        ctx.address_cell().unsync_address()
    }
}

impl<A> ActorAddress<A, (Address<A>, SyncAddress<A>)> for A where A: Actor {
    fn get(ctx: &mut Context<A>) -> (Address<A>, SyncAddress<A>) {
        (ctx.address_cell().unsync_address(), ctx.address_cell().sync_address())
    }
}

impl<A> ActorAddress<A, ()> for A where A: Actor {
    fn get(_: &mut Context<A>) -> () {
        ()
    }
}


pub trait Subscriber<M: 'static> {

    /// Buffered message
    fn send(&self, msg: M) -> Result<(), M>;

}

/// Address of the actor `A`.
/// Actor has to run in the same thread as owner of the address.
pub struct Address<A> where A: Actor {
    tx: unsync::UnboundedSender<ContextProtocol<A>>
}

impl<A> Clone for Address<A> where A: Actor {
    fn clone(&self) -> Self {
        Address{tx: self.tx.clone() }
    }
}

impl<A> Address<A> where A: Actor {

    pub(crate) fn new(sender: unsync::UnboundedSender<ContextProtocol<A>>) -> Address<A> {
        Address{tx: sender}
    }

    /// Indicates if address is still connected to the actor.
    pub fn connected(&self) -> bool {
        self.tx.connected()
    }

    /// Send message `M` to actor `A`.
    pub fn send<M: 'static>(&self, msg: M) where A: Handler<M>
    {
        let _ = self.tx.unbounded_send(
            ContextProtocol::Envelope(Envelope::local(msg, None)));
    }

    /// Send message to actor `A` and asyncronously wait for response.
    pub fn call<B: Actor, M>(&self, msg: M) -> Request<A, B, M>
        where A: Handler<M>,
              M: 'static
    {
        let (tx, rx) = channel();
        let _ = self.tx.unbounded_send(
            ContextProtocol::Envelope(Envelope::local(msg, Some(tx))));

        Request::local(rx)
    }

    /// Send message to actor `A` and asyncronously wait for response.
    pub fn call_fut<M>(&self, msg: M) -> Receiver<Result<A::Item, A::Error>>
        where A: Handler<M>,
              M: 'static
    {
        let (tx, rx) = channel();
        let _ = self.tx.unbounded_send(
            ContextProtocol::Envelope(Envelope::local(msg, Some(tx))));

        rx
    }

    /// Upgrade address to SyncAddress.
    pub fn upgrade(&self) -> Receiver<SyncAddress<A>> {
        let (tx, rx) = channel();
        let _ = self.tx.unbounded_send(
            ContextProtocol::Upgrade(tx));

        rx
    }

    /// Get `Subscriber` for specific message type
    pub fn subscriber<M: 'static>(&self) -> Box<Subscriber<M>>
        where A: Handler<M>
    {
        Box::new(self.clone())
    }
}

impl<A, M> Subscriber<M> for Address<A>
    where A: Actor + Handler<M>,
          M: 'static
{

    fn send(&self, msg: M) -> Result<(), M> {
        if self.connected() {
            self.send(msg);
            Ok(())
        } else {
            Err(msg)
        }
    }
}

/// Address of the actor `A`. Actor can run in differend thread.
pub struct SyncAddress<A> where A: Actor {
    tx: sync::UnboundedSender<Envelope<A>>,
    closed: Cell<bool>,
}

impl<A> Clone for SyncAddress<A> where A: Actor {
    fn clone(&self) -> Self {
        SyncAddress{tx: self.tx.clone(), closed: self.closed.clone()}
    }
}

impl<A> ActorAddress<A, SyncAddress<A>> for A where A: Actor {

    fn get(ctx: &mut Context<A>) -> SyncAddress<A> {
        ctx.address_cell().sync_address()
    }
}

impl<A> SyncAddress<A> where A: Actor {

    pub(crate) fn new(sender: sync::UnboundedSender<Envelope<A>>) -> SyncAddress<A> {
        SyncAddress{tx: sender, closed: Cell::new(false)}
    }

    /// Indicates if address is still connected to the actor.
    pub fn connected(&self) -> bool {
        !self.closed.get()
    }

    /// Send message `M` to actor `A`. Message cold be sent to actor running in
    /// different thread.
    pub fn send<M: 'static + Send>(&self, msg: M)
        where A: Handler<M> + ResponseType<M>,
              A::Item: Send,
              A::Error: Send,
    {
        if self.tx.unbounded_send(Envelope::remote(msg, None)).is_err()
        {
            self.closed.set(true)
        }
    }

    /// Send message to actor `A` and asyncronously wait for response.
    pub fn call<B: Actor, M: 'static + Send>(&self, msg: M) -> Request<A, B, M>
        where A: Handler<M>,
              A::Item: Send,
              A::Error: Send,
    {
        let (tx, rx) = sync_channel();
        if self.tx.unbounded_send(Envelope::remote(msg, Some(tx))).is_err()
        {
            self.closed.set(true)
        }

        Request::remote(rx)
    }

    /// Send message to actor `A` and asyncronously wait for response.
    pub fn call_fut<M>(&self, msg: M) -> SyncReceiver<Result<A::Item, A::Error>>
        where A: Handler<M>,
              M: Send + 'static,
              A::Item: Send,
              A::Error: Send,
    {
        let (tx, rx) = sync_channel();
        if self.tx.unbounded_send(Envelope::remote(msg, Some(tx))).is_err() {
            self.closed.set(true)
        }

        rx
    }

    /// Get `Subscriber` for specific message type
    pub fn subscriber<M: 'static + Send>(&self) -> Box<Subscriber<M> + Send>
        where A: Handler<M>,
              A::Item: Send,
              A::Error: Send,
    {
        Box::new(self.clone())
    }
}

impl<A, M> Subscriber<M> for SyncAddress<A>
    where A: Actor + Handler<M>,
          A::Item: Send,
          A::Error: Send,
          M: Send + 'static
{
    fn send(&self, msg: M) -> Result<(), M> {
        if self.connected() {
            self.send(msg);
            Ok(())
        } else {
            Err(msg)
        }
    }
}
