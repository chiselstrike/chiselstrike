use anyhow::{ensure, Result};
use futures_core::ready;
use reqwest::{Response, Url};
use std::panic;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Drop the extension (.d.ts/.ts/.js) from a path
pub fn without_extension(path: &str) -> &str {
    for suffix in [".d.ts", ".ts", ".js"] {
        if let Some(s) = path.strip_suffix(suffix) {
            return s;
        }
    }
    path
}

/// Simple wrapper over request::get that errors if the response status
/// is not success.
pub async fn get_ok(url: Url) -> Result<Response> {
    let res = reqwest::get(url).await?;
    ensure!(res.status().is_success(), "HTTP request failed");
    Ok(res)
}

pub fn make_signal_channel() -> (async_channel::Sender<()>, async_channel::Receiver<()>) {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_hook(info);
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).unwrap();
    }));
    async_channel::bounded(1)
}

/// Task that should not panic or be cancelled.
///
/// Does two things differently to `tokio::task::JoinHandle`:
/// 1. Aborts the task when dropped.
/// 2. Panics if the task panicked or was cancelled.
///
/// Can be `.await`-ed like a normal join handle.
#[derive(Debug)]
pub struct TaskHandle<T>(pub tokio::task::JoinHandle<T>);

impl<T> Future for TaskHandle<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match ready!(Pin::new(&mut self.get_mut().0).poll(cx)) {
            Ok(result) => Poll::Ready(result),
            Err(err) => {
                if err.is_cancelled() {
                    panic!("Task was cancelled")
                } else {
                    panic::resume_unwind(err.into_panic());
                }
            },
        }
    }
}

impl<T> Drop for TaskHandle<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Task that should not panic, but might be cancelled.
///
/// Does three things differently to `tokio::task::JoinHandle`:
/// 1. Aborts the task when dropped.
/// 2. Panics if the task panicked.
/// 3. Returns `None` if the task was cancelled.
#[derive(Debug)]
pub struct CancellableTaskHandle<T>(pub tokio::task::JoinHandle<T>);

impl<T> Future for CancellableTaskHandle<T> {
    type Output = Option<T>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match ready!(Pin::new(&mut self.get_mut().0).poll(cx)) {
            Ok(result) => Poll::Ready(Some(result)),
            Err(err) => {
                if err.is_cancelled() {
                    Poll::Ready(None)
                } else {
                    panic::resume_unwind(err.into_panic());
                }
            },
        }
    }
}

impl<T> Drop for CancellableTaskHandle<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

