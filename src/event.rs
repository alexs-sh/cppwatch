use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, Receiver, Sender};

#[derive(Debug)]
pub enum ExecutorEvent {
    Success((u64, Vec<StepData>)),
    Fail((u64, Vec<StepData>)),
}

#[derive(Clone, Debug)]
pub struct StepData {
    pub status: bool,
    pub start_at: Instant,
    pub stop_at: Instant,
    pub name: String,
}

pub fn make_channel() -> (Sender<ExecutorEvent>, Receiver<ExecutorEvent>) {
    mpsc::channel(16)
}

impl StepData {
    pub fn get_duration(&self) -> Duration {
        self.stop_at - self.start_at
    }
}
