#[cfg(not(feature = "tokio"))]
use crossbeam_channel::{unbounded as channel, Sender, Receiver};
#[cfg(feature = "tokio")]
use std::{future::Future, pin::Pin, task::{Context, Poll}};
use anyhow::{Error, Result};
use serde_json::Value;
#[cfg(feature = "tokio")]
use tokio::sync::oneshot::{channel, Sender, Receiver};

pub struct Tx(Sender<Result<Value>>);
pub struct Rx(Receiver<Result<Value>>);

pub fn oneshot() -> (Tx, Rx) {
    let (tx, rx) = channel();
    (Tx(tx), Rx(rx))
}

impl Tx {
    pub fn send(self, result: Result<Value, Value>) {
        let result = result.map_err(|value| {
            match value {
                Value::String(s) => Error::msg(s),
                value            => Error::msg(value),
            }
        });

        match self.0.send(result) {
            Ok(()) => (),
            Err(_) => (),
        }
    }
}

#[cfg(not(feature = "tokio"))]
impl Rx {
    pub fn recv(self) -> Result<Value> {
        self.0.recv()?
    }
}

#[cfg(feature = "tokio")]
impl Rx {
    pub fn recv(self) -> Result<Value> {
        self.0.blocking_recv()?
    }
}

#[cfg(feature = "tokio")]
impl Future for Rx {
    type Output = Result<Value>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.0).poll(cx) {
            Poll::Ready(Ok(r))  => Poll::Ready(r),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
            Poll::Pending       => Poll::Pending,
        }
    }
}
