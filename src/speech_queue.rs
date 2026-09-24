use anyhow::{bail, Result};
use std::process::Stdio;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

const CAPACITY: usize = 64;

struct Job {
    id: u64,
    args: super::SpeakArgs,
}

#[derive(Clone)]
pub struct SpeechQueue {
    sender: mpsc::Sender<Job>,
    next_id: Arc<AtomicU64>,
}

impl SpeechQueue {
    pub fn start() -> (Self, oneshot::Sender<()>, JoinHandle<()>) {
        let (sender, mut receiver) = mpsc::channel::<Job>(CAPACITY);
        let (stop, mut stopping) = oneshot::channel();
        let worker = tokio::spawn(async move {
            loop {
                let job = tokio::select! {
                    biased;
                    _ = &mut stopping => break,
                    job = receiver.recv() => match job {
                        Some(job) => job,
                        None => break,
                    },
                };
                let mut command = Command::new("say");
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit())
                    .kill_on_drop(true);
                if let Some(voice) = job.args.voice {
                    command.arg("-v").arg(voice);
                }
                if let Some(speed) = job.args.speed {
                    command.arg("-r").arg(speed.to_string());
                }
                command.arg("--").arg(job.args.text);
                match command.spawn() {
                    Ok(mut child) => {
                        tokio::select! {
                            biased;
                            _ = &mut stopping => {
                                // kill() also waits, reaping the child before shutdown.
                                if let Err(error) = child.kill().await {
                                    eprintln!("Speech job {} shutdown: {}", job.id, error);
                                }
                                break;
                            }
                            result = child.wait() => match result {
                                Ok(status) if status.success() => {},
                                result => eprintln!("Speech job {} failed: {:?}", job.id, result),
                            }
                        }
                    }
                    Err(error) => eprintln!("Speech job {} failed to start: {}", job.id, error),
                }
            }
        });
        (
            Self {
                sender,
                next_id: Arc::new(AtomicU64::new(1)),
            },
            stop,
            worker,
        )
    }

    pub fn enqueue(&self, args: super::SpeakArgs) -> Result<u64> {
        if args.text.trim().is_empty() {
            bail!("Speech text must not be empty");
        }
        if args.speed == Some(0) {
            bail!("Speech speed must be greater than zero");
        }
        if args.voice.as_ref().is_some_and(|v| v.trim().is_empty()) {
            bail!("Speech voice must not be empty");
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.sender
            .try_send(Job { id, args })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => anyhow::anyhow!("Speech queue is full"),
                mpsc::error::TrySendError::Closed(_) => anyhow::anyhow!("Speech queue is closed"),
            })?;
        Ok(id)
    }
}
