use futures::stream::{FuturesUnordered, Stream, FusedStream};
use guard::guard;
use parking_lot::Mutex;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll, Waker};

#[derive(Debug, Clone)]
pub struct Nursery<Fut> {
    state: Arc<Mutex<NurseryState<Fut>>>,
}

#[derive(Debug)]
pub struct NurseryStream<Fut> {
    state: Weak<Mutex<NurseryState<Fut>>>,
}

#[derive(Debug)]
struct NurseryState<Fut> {
    futures: FuturesUnordered<Fut>,
    waker: Option<Waker>,
}

impl<Fut> Nursery<Fut> {
    pub fn new() -> (Nursery<Fut>, NurseryStream<Fut>) {
        let state = Arc::new(Mutex::new(NurseryState {
            futures: FuturesUnordered::new(),
            waker: None,
        }));
        let stream = NurseryStream { state: Arc::downgrade(&state) };
        let nursery = Nursery { state };
        (nursery, stream)
    }

    pub fn nurse(&self, fut: Fut) {
        let mut state = self.state.lock();
        state.futures.push(fut);
        if let Some(waker) = state.waker.take() {
            waker.wake()
        }
    }
}

/*
impl<T> Nursery<TaskHandle<T>> {
    pub fn spawn<Fut>(&self, fut: Fut)
        where Fut: Future<Output = T> + Send + 'static,
              T: Send + 'static
    {
        self.nurse(TaskHandle(tokio::task::spawn(fut)))
    }
}

impl<T> Nursery<CancellableTaskHandle<T>> {
    pub fn spawn<Fut>(&self, fut: Fut)
        where Fut: Future<Output = T> + Send + 'static,
              T: Send + 'static
    {
        self.nurse(CancellableTaskHandle(tokio::task::spawn(fut)))
    }
}
*/

impl<Fut, T> Stream for NurseryStream<Fut>
    where Fut: Future<Output = T>
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        guard!{let Some(state) = Weak::upgrade(&this.state) else {
            return Poll::Ready(None);
        }};

        let mut state = state.lock();
        if let Poll::Ready(Some(x)) = Pin::new(&mut state.futures).poll_next(cx) {
            return Poll::Ready(Some(x));
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl<Fut, T> FusedStream for NurseryStream<Fut>
    where Fut: Future<Output = T>
{
    fn is_terminated(&self) -> bool {
        self.state.strong_count() == 0
    }
}
