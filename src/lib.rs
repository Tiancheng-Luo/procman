#![feature(await_macro, async_await)]

use std::collections::HashMap;
use std::collections::VecDeque;
use std::error;
use std::fmt;
use std::io::Read;
use std::io::{Error, ErrorKind, Result};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::str;
use std::string::String;
use std::sync::Arc;
use std::sync::RwLock;
use std::thread;
use std::time;

type ProcessTable = Arc<RwLock<HashMap<String, Arc<RwLock<ProcessControl>>>>>;
type EventQueue = Arc<RwLock<VecDeque<ProcessEvent>>>;

/// A `ProcessManager` manages a family of processes, where notable events in
/// the life of those processes get reported to a "directing actor".
#[derive(Clone, Default)]
pub struct ProcessManager {
    processes: ProcessTable,
}

struct ProcessControl {
    child: Child,
    event_queue: EventQueue,
}

#[derive(Clone, Copy, Debug)]
pub enum HandleType {
    StdInput,
    StdOutput,
    StdError,
}

#[derive(Debug)]
pub enum ProcessError {
    ErrorWaiting(Error),
    ErrorReading(Error),
    ErrorHandling(Error),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessError::ErrorWaiting(e) => write!(f, "ErrorWaiting: {}", e),
            ProcessError::ErrorReading(e) => write!(f, "ErrorReading: {}", e),
            ProcessError::ErrorHandling(e) => write!(f, "ErrorHandling: {}", e),
        }
    }
}

impl error::Error for ProcessError {}

#[derive(Debug)]
pub enum ManagerError {
    ProcessUnknown,
}

const MAX_LINE: usize = 8192;

#[derive(Debug)]
pub enum ProcessEvent {
    Exited(ExitStatus),
    Error(ProcessError),
    Output(HandleType, Vec<u8>, usize),
}

impl fmt::Display for ProcessEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessEvent::Exited(status) => write!(f, "Exited({})", status),
            ProcessEvent::Error(err) => write!(f, "Error({})", err),
            ProcessEvent::Output(handle, bytes, len) => write!(
                f,
                "Output({:?}, {:?}, {})",
                handle,
                str::from_utf8(&bytes[0..*len]),
                len
            ),
        }
    }
}

impl ProcessManager {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn run_director_with_intercept<F>(&self, on_event: F) -> Result<()>
    where
        F: Fn(ProcessEvent, &mut dyn FnMut(ProcessEvent)),
    {
        loop {
            thread::sleep(time::Duration::from_millis(200));

            let mut to_remove: Vec<String> = Vec::new();

            if self.processes.read().unwrap().len() == 0 {
                return Ok(());
            } else {
                for (name, ctl) in self.processes.write().unwrap().iter_mut() {
                    if let Some(ev) = (*ctl)
                        .write()
                        .unwrap()
                        .event_queue
                        .write()
                        .unwrap()
                        .pop_front()
                    {
                        on_event(ev, &mut |ev| {
                            if let ProcessEvent::Exited(_code) = ev {
                                to_remove.push(name.to_string())
                            }
                        })
                    }
                }

                for name in to_remove {
                    let mut procs = self.processes.write().unwrap();
                    procs.remove(&name);
                }
            }
        }
    }

    pub fn run_director(&self) -> Result<()> {
        self.run_director_with_intercept(|ev, k: &mut dyn FnMut(ProcessEvent)| k(ev))
    }

    pub fn run_process_with_intercept<F>(
        &self,
        name: String,
        command: &mut Command,
        on_event: F,
    ) -> Result<()>
    where
        F: Fn(ProcessEvent, &dyn Fn(ProcessEvent) -> Result<()>) -> Result<()>,
    {
        // Remember some details about `config`, since we will be moving it.
        let name: String = name.to_string();

        // Spawn the child process, which begins running immediately.
        let child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut ctl = ProcessControl {
            child,
            event_queue: Default::default(),
        };

        // Record the command in our "process table", and if we cannot because
        // of a name overlap, kill both the old and new processes and report
        // the error.
        let ctl = self
            .processes
            .write()
            .unwrap()
            .entry(name.to_string())
            .and_modify(|e| {
                (*e).write().unwrap().child.kill().unwrap_or_default();
                ctl.child.kill().unwrap_or_default();
                panic!("Overwriting existing process with name {}", name)
            })
            .or_insert_with(|| Arc::new(RwLock::new(ctl)))
            .clone();

        let mut buf: [u8; MAX_LINE] = [0; MAX_LINE];
        let on_event = |ctl: &ProcessControl, ev: ProcessEvent| -> Result<()> {
            if let Err(e) = (on_event)(ev, &move |ev| {
                ctl.event_queue.write().unwrap().push_back(ev);
                Ok(())
            }) {
                ctl.event_queue
                    .write()
                    .unwrap()
                    .push_back(ProcessEvent::Error(ProcessError::ErrorHandling(e)))
            };
            Ok(())
        };

        loop {
            thread::sleep(time::Duration::from_millis(200));

            let mut ctl = ctl.write().unwrap();

            // Check whether this is output to be read.
            if let Some(h) = &mut ctl.child.stdout {
                match h.read(&mut buf) {
                    Ok(len) => (on_event)(
                        &ctl,
                        ProcessEvent::Output(HandleType::StdOutput, buf.to_vec(), len),
                    ),
                    Err(e) => (on_event)(&ctl, ProcessEvent::Error(ProcessError::ErrorReading(e))),
                }
            } else {
                Ok(())
            }?;

            if let Some(h) = &mut ctl.child.stderr {
                match h.read(&mut buf) {
                    Ok(len) => (on_event)(
                        &ctl,
                        ProcessEvent::Output(HandleType::StdError, buf.to_vec(), len),
                    ),
                    Err(e) => (on_event)(&ctl, ProcessEvent::Error(ProcessError::ErrorReading(e))),
                }
            } else {
                Ok(())
            }?;

            let result: Result<()> = match ctl.child.try_wait() {
                Ok(None) => Ok(()),
                Ok(Some(status)) => return (on_event)(&ctl, ProcessEvent::Exited(status)),
                Err(e) => {
                    return (on_event)(&ctl, ProcessEvent::Error(ProcessError::ErrorWaiting(e)))
                }
            };

            result?
        }
    }

    pub fn run_process(&self, name: String, command: &mut Command) -> Result<()> {
        self.run_process_with_intercept(
            name,
            command,
            |ev, k: &dyn Fn(ProcessEvent) -> Result<()>| k(ev),
        )
    }

    pub fn stop_process(&mut self, name: &str) -> Result<()> {
        if let Some(v) = self.processes.write().unwrap().remove(name) {
            v.write().unwrap().child.kill()?;
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::Other,
                format!("Could not find entry {} to be stopped", name),
            ))
        }
    }
}
