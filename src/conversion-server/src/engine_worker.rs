use std::{
    error::Error,
    fmt,
    sync::mpsc::{self, Sender},
    thread,
};

pub trait ConversionEngine: Send + 'static {
    fn handle(&mut self, request: String) -> Result<String, String>;
}

#[derive(Debug)]
pub enum EngineWorkerError {
    WorkerStopped,
    Engine(String),
}

impl fmt::Display for EngineWorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkerStopped => write!(f, "conversion engine worker stopped"),
            Self::Engine(message) => write!(f, "conversion engine error: {message}"),
        }
    }
}

impl Error for EngineWorkerError {}

enum EngineCommand {
    Handle {
        request: String,
        reply: Sender<Result<String, String>>,
    },
}

/// Owns the conversion engine on one dedicated thread.
///
/// The Windows TSF is loaded into many application processes, so the Swift
/// conversion engine must not live in the TSF DLL. The conversion-server owns
/// one worker instead, and all stateful engine calls are serialized here.
#[derive(Clone)]
pub struct EngineWorker {
    sender: Sender<EngineCommand>,
}

impl EngineWorker {
    pub fn spawn<E: ConversionEngine>(mut engine: E) -> Self {
        let (sender, receiver) = mpsc::channel::<EngineCommand>();

        thread::Builder::new()
            .name("azookey-converter-engine".into())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    match command {
                        EngineCommand::Handle { request, reply } => {
                            let _ = reply.send(engine.handle(request));
                        }
                    }
                }
            })
            .expect("failed to start conversion engine worker");

        Self { sender }
    }

    pub fn handle(&self, request: String) -> Result<String, EngineWorkerError> {
        let (reply_sender, reply_receiver) = mpsc::channel();
        self.sender
            .send(EngineCommand::Handle {
                request,
                reply: reply_sender,
            })
            .map_err(|_| EngineWorkerError::WorkerStopped)?;

        reply_receiver
            .recv()
            .map_err(|_| EngineWorkerError::WorkerStopped)?
            .map_err(EngineWorkerError::Engine)
    }
}

/// Temporary engine used while the Swift bridge is being wired.
///
/// Keeping this behind ConversionEngine means replacing it with the actual
/// azooKey Desktop engine does not change the IPC or TSF layers.
pub struct EchoEngine;

impl ConversionEngine for EchoEngine {
    fn handle(&mut self, request: String) -> Result<String, String> {
        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CountingEngine {
        count: usize,
    }

    impl ConversionEngine for CountingEngine {
        fn handle(&mut self, request: String) -> Result<String, String> {
            self.count += 1;
            Ok(format!("{}:{request}", self.count))
        }
    }

    #[test]
    fn worker_keeps_one_serialized_engine_instance() {
        let worker = EngineWorker::spawn(CountingEngine { count: 0 });

        assert_eq!(worker.handle("a".into()).unwrap(), "1:a");
        assert_eq!(worker.handle("b".into()).unwrap(), "2:b");
        assert_eq!(worker.handle("c".into()).unwrap(), "3:c");
    }
}
