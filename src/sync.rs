use crate::queue::Queue;
use crate::{
    clipboard::{Clipboard, Snapshot},
    config::{Config, Mode},
    protocol::{Protocol, normalize},
};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;
use uuid::Uuid;

#[derive(Clone, PartialEq, Eq)]
enum Current {
    Text([u8; 32]),
    Empty,
    NonText,
}
fn current(snapshot: &Snapshot) -> Option<Current> {
    match snapshot {
        Snapshot::Text(text) => Some(Current::Text(
            Sha256::digest(normalize(text).as_bytes()).into(),
        )),
        Snapshot::Empty => Some(Current::Empty),
        Snapshot::NonText => Some(Current::NonText),
        Snapshot::Unavailable => None,
    }
}
#[derive(Clone)]
pub struct Publication {
    pub body: String,
    pub timeout: Duration,
}
pub enum PublishResult {
    Accepted,
    Retry { after: Option<Duration> },
    Rejected,
}

#[derive(Clone)]
pub struct Receiver {
    protocol: Protocol,
    writes: Arc<Mutex<Queue>>,
    generation: Arc<AtomicU64>,
}
impl Receiver {
    pub fn receive(&self, frame: &str) -> Result<()> {
        self.receive_at(frame, Instant::now())
    }
    fn receive_at(&self, frame: &str, received: Instant) -> Result<()> {
        if let Some(text) = self.protocol.decode(frame)? {
            self.writes.lock().expect("write queue poisoned").push_at(
                text,
                self.generation.load(Ordering::SeqCst),
                received,
            );
        }
        Ok(())
    }
}

pub struct Coordinator<C> {
    config: Config,
    protocol: Protocol,
    clipboard: C,
    current: Option<Current>,
    generation: u64,
    send: Queue,
    writes: Arc<Mutex<Queue>>,
    shared_generation: Arc<AtomicU64>,
    publishing: bool,
    cooldown: Instant,
}
impl<C: Clipboard> Coordinator<C> {
    pub fn new(config: Config, clipboard: C) -> Result<Self> {
        let protocol = Protocol::new(
            &config.topic,
            &Uuid::new_v4().to_string(),
            config.max_message,
        )?;
        let send = Queue::new(config.send.clone(), "publish");
        let writes = Queue::new(config.write.clone(), "write");
        Ok(Self {
            config,
            protocol,
            clipboard,
            current: None,
            generation: 0,
            send,
            writes: Arc::new(Mutex::new(writes)),
            shared_generation: Arc::new(AtomicU64::new(0)),
            publishing: false,
            cooldown: Instant::now(),
        })
    }
    pub fn clipboard_mut(&mut self) -> &mut C {
        &mut self.clipboard
    }
    pub fn protocol(&self) -> &Protocol {
        &self.protocol
    }
    pub fn receiver(&self) -> Receiver {
        Receiver {
            protocol: self.protocol.clone(),
            writes: self.writes.clone(),
            generation: self.shared_generation.clone(),
        }
    }
    pub async fn observe(&mut self) -> bool {
        match self.clipboard.read().await {
            Ok(snapshot) => {
                let Some(state) = current(&snapshot) else {
                    self.current = None;
                    return false;
                };
                if self.current.as_ref() == Some(&state) {
                    return true;
                }
                let baseline = self.current.is_none();
                self.current = Some(state);
                self.generation += 1;
                self.shared_generation
                    .store(self.generation, Ordering::SeqCst);
                if !baseline
                    && self.config.mode == Mode::Bidirectional
                    && let Snapshot::Text(text) = snapshot
                {
                    match self.protocol.encode(&text) {
                        Ok(body) => self.send.push(body, 0),
                        Err(e) => log::warn!("publish rejected: {e}"),
                    }
                }
                true
            }
            Err(error) => {
                log::warn!("clipboard observation failed; retaining last valid state: {error}");
                false
            }
        }
    }
    pub async fn receive(&mut self, frame: &str) -> Result<()> {
        let received = Instant::now();
        if self.protocol.decode(frame)?.is_some() {
            self.observe().await;
        }
        self.receiver().receive_at(frame, received)
    }
    pub async fn write_next(&mut self) {
        {
            let mut writes = self.writes.lock().expect("write queue poisoned");
            writes.expire();
            while writes
                .jobs
                .front()
                .is_some_and(|job| job.generation != self.generation)
            {
                log::warn!("write cancelled by local change");
                writes.pop();
            }
            let Some(job) = writes.jobs.front_mut() else {
                return;
            };
            if Instant::now() < job.ready {
                return;
            }
            // Reserve the head across the fresh read: concurrent ingress may
            // evict only waiting jobs, never this operation or its accounting.
            job.active = true;
        }
        let observed = self.observe().await;
        let (text, expected, timeout) = {
            let mut writes = self.writes.lock().expect("write queue poisoned");
            let job = writes.jobs.front_mut().expect("reserved write head");
            if job.deadline <= Instant::now()
                || (job.attempts > 0 && job.generation != self.generation)
            {
                log::warn!("write expired or cancelled by local change");
                writes.pop();
                return;
            }
            // First delivery processes newly discovered local content before
            // registering its target. Previously started retries never do this.
            job.generation = self.generation;
            if !observed && self.config.mode == Mode::Bidirectional {
                job.active = false;
                return;
            }
            let expected = current(&Snapshot::Text(job.text.clone()));
            if observed && expected == self.current {
                writes.pop();
                return;
            }
            job.attempts += 1;
            (
                job.text.clone(),
                expected,
                self.config
                    .write
                    .timeout
                    .min(job.deadline.saturating_duration_since(Instant::now())),
            )
        };
        let result = self.clipboard.write(&text, timeout).await;
        let mut writes = self.writes.lock().expect("write queue poisoned");
        match result {
            Ok(()) => {
                self.current = expected;
                writes.pop();
                log::info!("clipboard write accepted");
            }
            Err(crate::clipboard::WriteError::Temporary(reason)) => {
                log::warn!("clipboard temporarily unavailable: {reason}");
                writes.jobs.front_mut().unwrap().active = false;
                writes.retry();
            }
            Err(crate::clipboard::WriteError::Permanent(reason)) => {
                log::warn!("clipboard write rejected: {reason}");
                writes.pop();
            }
        }
    }
    pub fn next_publish(&mut self) -> Option<Publication> {
        if self.publishing {
            return None;
        }
        self.send.expire();
        if Instant::now() < self.cooldown {
            return None;
        }
        let job = self.send.jobs.front_mut()?;
        if Instant::now() < job.ready {
            return None;
        }
        job.attempts += 1;
        self.publishing = true;
        Some(Publication {
            body: job.text.clone(),
            timeout: self
                .config
                .send
                .timeout
                .min(job.deadline.saturating_duration_since(Instant::now())),
        })
    }
    pub fn published(&mut self, result: PublishResult) {
        self.publishing = false;
        match result {
            PublishResult::Accepted => {
                self.send.pop();
                log::info!("publish accepted by server");
            }
            PublishResult::Rejected => {
                self.send.pop();
                log::warn!("publish rejected permanently");
            }
            PublishResult::Retry { after } => {
                if let Some(delay) = after {
                    self.cooldown = self.cooldown.max(Instant::now() + delay);
                }
                log::warn!("publish failed; bounded retry scheduled");
                self.send.retry();
            }
        }
    }
}
