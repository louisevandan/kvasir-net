use p4_adapter::node_adapter::*;
use p4_protocol::event::Endpoint;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
};
use tokio::sync::{Notify, mpsc};

pub(crate) struct Input {
    pub completion: Option<RetainedCompletion>,
    pub bytes: Arc<AtomicUsize>,
    pub cost: usize,
}
impl Drop for Input {
    fn drop(&mut self) {
        self.bytes.fetch_sub(self.cost, Ordering::AcqRel);
    }
}

pub struct HfNodeAdapter {
    pub(crate) sender: Option<mpsc::Sender<Input>>,
    pub(crate) mailbox: Arc<CompletionMailbox>,
    pub(crate) state: Arc<Mutex<String>>,
    pub(crate) bytes: Arc<AtomicUsize>,
    pub(crate) limit: usize,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) notify: Arc<Notify>,
    pub(crate) thread: Mutex<Option<JoinHandle<()>>>,
}

impl HfNodeAdapter {
    pub fn new(
        endpoint: Endpoint,
        input_capacity: usize,
        completion_capacity: usize,
        retained_capacity: usize,
        retained_bytes: usize,
    ) -> Result<Self, String> {
        endpoint.validate().map_err(|e| e.to_string())?;
        if input_capacity == 0 || input_capacity > 65536 {
            return Err("invalid input capacity".into());
        }
        let (publisher, mailbox) =
            completion_mailbox_with_limits(completion_capacity, retained_capacity, retained_bytes)
                .map_err(|e| format!("completion storage: {e:?}"))?;
        let (sender, receiver) = mpsc::channel(input_capacity);
        let state = Arc::new(Mutex::new("empty".into()));
        let stop = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        let (s, flag, wake, storage) =
            (state.clone(), stop.clone(), notify.clone(), mailbox.clone());
        let thread = std::thread::Builder::new()
            .name("p4-hf-retained".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(crate::lifecycle::run(
                    endpoint, receiver, publisher, storage, s, flag, wake,
                ));
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            sender: Some(sender),
            mailbox,
            state,
            bytes: Arc::new(AtomicUsize::new(0)),
            limit: retained_bytes,
            stop,
            notify,
            thread: Mutex::new(Some(thread)),
        })
    }
}

impl Drop for HfNodeAdapter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.notify.notify_one();
        self.sender.take();
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}
