//! Serial background execution for owned Herdr operations.
//!
//! Poll `try_recv` from the UI thread. After a failure, reconcile Herdr state
//! before calling `resume`; commands ahead of that resume are skipped, not replayed.
//! Command errors are returned unchanged, so callers must not include user input
//! in them or display them without appropriate sanitization.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

type Command = Box<dyn FnOnce() -> Result<(), String> + Send + 'static>;

enum Message {
    Command(Command),
    Resume,
}

/// The outcome of one accepted command, in submission order.
#[derive(Debug, PartialEq, Eq)]
pub enum Completion {
    Finished(Result<(), String>),
    /// A previous command failed and no subsequent resume preceded this command.
    Skipped,
}

/// A single ordered worker. Dropping it closes the queue; an in-flight or next
/// queued command can still run. Commands must not panic.
pub struct CommandWorker {
    sender: Sender<Message>,
    completions: Receiver<Completion>,
}

impl CommandWorker {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let (completion_sender, completions) = mpsc::channel();
        thread::spawn(move || {
            let mut paused = false;
            while let Ok(message) = receiver.recv() {
                match message {
                    Message::Resume => paused = false,
                    Message::Command(command) => {
                        let completion = if paused {
                            Completion::Skipped
                        } else {
                            let result = command();
                            paused = result.is_err();
                            Completion::Finished(result)
                        };
                        if completion_sender.send(completion).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Self {
            sender,
            completions,
        }
    }

    /// Queue a command without waiting for it to execute.
    /// An error means the worker is no longer available.
    pub fn submit(
        &self,
        command: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) -> Result<(), &'static str> {
        self.sender
            .send(Message::Command(Box::new(command)))
            .map_err(|_| "command worker unavailable")
    }

    /// Queue a resume after the caller has reconciled state.
    pub fn resume(&self) -> Result<(), &'static str> {
        self.sender
            .send(Message::Resume)
            .map_err(|_| "command worker unavailable")
    }

    /// Return the next completion without waiting.
    pub fn try_recv(&self) -> Result<Completion, TryRecvError> {
        self.completions.try_recv()
    }
}

impl Default for CommandWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn executes_and_completes_in_order() {
        let worker = CommandWorker::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        for value in 0..3 {
            let seen = Arc::clone(&seen);
            assert!(worker
                .submit(move || {
                    seen.lock().unwrap_or_else(|e| e.into_inner()).push(value);
                    Ok(())
                })
                .is_ok());
        }
        for _ in 0..3 {
            assert_eq!(
                worker.completions.recv_timeout(Duration::from_secs(2)),
                Ok(Completion::Finished(Ok(())))
            );
        }
        assert_eq!(
            *seen.lock().unwrap_or_else(|e| e.into_inner()),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn failure_skips_queued_commands_until_resume() {
        let worker = CommandWorker::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (release, wait) = mpsc::channel();
        assert!(worker
            .submit(move || {
                let _ = wait.recv();
                Err("failed".into())
            })
            .is_ok());
        let skipped = Arc::clone(&seen);
        assert!(worker
            .submit(move || {
                skipped.lock().unwrap_or_else(|e| e.into_inner()).push(1);
                Ok(())
            })
            .is_ok());
        assert!(release.send(()).is_ok());
        assert_eq!(
            worker.completions.recv_timeout(Duration::from_secs(2)),
            Ok(Completion::Finished(Err("failed".into())))
        );
        assert_eq!(
            worker.completions.recv_timeout(Duration::from_secs(2)),
            Ok(Completion::Skipped)
        );
        assert!(worker.resume().is_ok());
        let resumed = Arc::clone(&seen);
        assert!(worker
            .submit(move || {
                resumed.lock().unwrap_or_else(|e| e.into_inner()).push(2);
                Ok(())
            })
            .is_ok());
        assert_eq!(
            worker.completions.recv_timeout(Duration::from_secs(2)),
            Ok(Completion::Finished(Ok(())))
        );
        assert_eq!(*seen.lock().unwrap_or_else(|e| e.into_inner()), vec![2]);
    }
}
