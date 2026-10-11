//! Stack owner for an embedded compiler. The former API transport supplies a
//! large handler stack; a Node/Tokio caller does not. One serial worker preserves
//! that correctness requirement without spawning a process or adding a wire format.
use crate::{ApiError, ApiResult};
use std::sync::{Mutex, mpsc};
use std::thread::{self, JoinHandle};

type Work = Box<dyn FnOnce() + Send>;
pub(super) struct Executor {
    sender: Mutex<Option<mpsc::Sender<Work>>>,
    worker: Option<JoinHandle<()>>,
}
impl Executor {
    pub(super) fn new() -> ApiResult<Self> {
        let (sender, receiver) = mpsc::channel::<Work>();
        let worker = thread::Builder::new()
            .name("tsrs-embedded".into())
            .stack_size(tsrs_api_transport::conn_async::DEFAULT_HANDLER_STACK_SIZE)
            .spawn(move || {
                while let Ok(work) = receiver.recv() {
                    work();
                }
            })
            .map_err(|e| ApiError::internal(format!("cannot start compiler worker: {e}")))?;
        Ok(Self {
            sender: Mutex::new(Some(sender)),
            worker: Some(worker),
        })
    }
    pub(super) fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce() -> ApiResult<T> + Send + 'static,
    ) -> ApiResult<T> {
        if self
            .worker
            .as_ref()
            .is_some_and(|w| w.thread().id() == thread::current().id())
        {
            return Err(ApiError::client(
                "reentrant embedded compiler request from a host callback",
            ));
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let work = Box::new(move || {
            let _ = sender.send(super::protect(f));
        });
        self.sender
            .lock()
            .unwrap()
            .as_ref()
            .ok_or_else(|| ApiError::client("compiler worker is closed"))?
            .send(work)
            .map_err(|e| ApiError::internal(format!("compiler worker stopped: {e}")))?;
        receiver
            .recv()
            .map_err(|e| ApiError::internal(format!("compiler worker stopped: {e}")))?
    }
}
impl Drop for Executor {
    fn drop(&mut self) {
        self.sender.get_mut().unwrap().take();
        if let Some(worker) = self.worker.take() {
            if worker.thread().id() != thread::current().id() {
                let _ = worker.join();
            }
        }
    }
}
