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
            let mut config = None;
            loop {
                let job = tokio::select! {
                    biased;
                    _ = &mut stopping => break,
                    job = receiver.recv() => match job {
                        Some(job) => job,
                        None => break,
                    },
                };
                let current = config.get_or_insert_with(super::load_config);
                let current = match current {
                    Ok(current) => current,
                    Err(error) => {
                        eprintln!(
                            "Speech job {} failed: invalid configuration: {}",
                            job.id, error
                        );
                        if receiver.is_empty() {
                            config = None;
                        }
                        continue;
                    }
                };
                let voice = current
                    .voice_for_locale(&job.args.locale)
                    .filter(|v| !v.trim().is_empty());
                let Some(voice) = voice else {
                    eprintln!(
                        "Speech job {} failed: no voice configured for {}",
                        job.id, job.args.locale
                    );
                    if receiver.is_empty() {
                        config = None;
                    }
                    continue;
                };
                let mut command = Command::new("say");
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit())
                    .kill_on_drop(true);
                command.arg("-v").arg(voice);
                command
                    .arg("-r")
                    .arg(job.args.speed.unwrap_or(current.rate.get()).to_string());
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
                // A drained queue ends this configuration snapshot's lifetime.
                if receiver.is_empty() {
                    config = None;
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
        if args.locale.trim().is_empty() {
            bail!("Speech locale must not be empty");
        }
        if args.text.trim().is_empty() {
            bail!("Speech text must not be empty");
        }
        if args.speed == Some(0) {
            bail!("Speech speed must be greater than zero");
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
