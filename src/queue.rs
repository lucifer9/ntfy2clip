use crate::config::Budget;
use std::collections::VecDeque;
use tokio::time::Instant;

pub(crate) struct Job {
    pub text: String,
    pub generation: u64,
    pub deadline: Instant,
    pub ready: Instant,
    pub attempts: u32,
    pub active: bool,
}
pub(crate) struct Queue {
    pub jobs: VecDeque<Job>,
    bytes: usize,
    pub budget: Budget,
    label: &'static str,
}
impl Queue {
    pub fn new(budget: Budget, label: &'static str) -> Self {
        Self {
            jobs: VecDeque::new(),
            bytes: 0,
            budget,
            label,
        }
    }
    pub fn push(&mut self, text: String, generation: u64) {
        self.push_at(text, generation, Instant::now());
    }
    pub fn push_at(&mut self, text: String, generation: u64, received: Instant) {
        if text.len() > self.budget.bytes {
            log::warn!("{} queue rejected oversized job", self.label);
            return;
        }
        while self.jobs.len() >= self.budget.messages
            || text.len() > self.budget.bytes.saturating_sub(self.bytes)
        {
            let Some(index) = self
                .jobs
                .iter()
                .position(|job| job.attempts == 0 && !job.active)
            else {
                log::warn!("{} queue rejected job: no evictable capacity", self.label);
                return;
            };
            let old = self.jobs.remove(index).unwrap();
            self.bytes -= old.text.len();
            log::warn!("{} queue evicted oldest unstarted job", self.label);
        }
        self.bytes += text.len();
        let now = Instant::now();
        self.jobs.push_back(Job {
            text,
            generation,
            deadline: received + self.budget.ttl,
            ready: now,
            attempts: 0,
            active: false,
        });
    }
    pub fn pop(&mut self) {
        if let Some(job) = self.jobs.pop_front() {
            self.bytes -= job.text.len();
        }
    }
    pub fn expire(&mut self) {
        while self
            .jobs
            .front()
            .is_some_and(|job| job.deadline <= Instant::now())
        {
            log::warn!("{} job expired", self.label);
            self.pop();
        }
    }
    pub fn retry(&mut self) {
        let Some(job) = self.jobs.front_mut() else {
            return;
        };
        if job.attempts >= self.budget.attempts {
            log::warn!("{} attempts exhausted", self.label);
            self.pop();
            return;
        }
        let multiplier = 1u32
            .checked_shl(job.attempts.saturating_sub(1))
            .unwrap_or(u32::MAX);
        let delay = self
            .budget
            .retry_base
            .saturating_mul(multiplier)
            .min(self.budget.retry_max);
        job.ready = Instant::now() + delay;
    }
}
