#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    env,
    fs::{self, File, OpenOptions},
    io,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use fs2::FileExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const STABLE_PROCESS_AGE: Duration = Duration::from_secs(30);
const INITIAL_RESTART_DELAY: Duration = Duration::from_secs(1);
const MAX_RESTART_DELAY: Duration = Duration::from_secs(30);

fn main() -> Result<()> {
    let Some(_lock) = HostLock::acquire()? else {
        return Ok(());
    };

    let executable_directory = env::current_exe()
        .context("could not resolve azooKey host executable")?
        .parent()
        .context("azooKey host has no executable directory")?
        .to_path_buf();

    let mut conversion_server =
        ManagedProcess::new("conversion-server.exe", executable_directory.clone());
    let mut candidate_ui = ManagedProcess::new("candidate-ui.exe", executable_directory);

    loop {
        conversion_server.tick();
        candidate_ui.tick();
        thread::sleep(HEALTH_POLL_INTERVAL);
    }
}

struct HostLock {
    _file: File,
}

impl HostLock {
    fn acquire() -> Result<Option<Self>> {
        let state_directory = host_state_directory();
        fs::create_dir_all(&state_directory)
            .context("failed to create azooKey host state directory")?;

        let lock_path = state_directory.join("host.lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("failed to open {}", lock_path.display()))?;

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error).context("failed to lock azooKey host instance"),
        }
    }
}

fn host_state_directory() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("azooKey")
        .join("Host")
}

struct ManagedProcess {
    executable_name: &'static str,
    directory: PathBuf,
    child: Option<Child>,
    started_at: Option<Instant>,
    next_restart_at: Instant,
    restart_delay: Duration,
}

impl ManagedProcess {
    fn new(executable_name: &'static str, directory: PathBuf) -> Self {
        Self {
            executable_name,
            directory,
            child: None,
            started_at: None,
            next_restart_at: Instant::now(),
            restart_delay: INITIAL_RESTART_DELAY,
        }
    }

    fn tick(&mut self) {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(None) => return,
                Ok(Some(_status)) => self.record_exit(),
                Err(_error) => self.record_exit(),
            }
        }

        if Instant::now() < self.next_restart_at {
            return;
        }

        match spawn_child(&self.directory, self.executable_name) {
            Ok(child) => {
                self.child = Some(child);
                self.started_at = Some(Instant::now());
            }
            Err(_error) => {
                self.schedule_restart(false);
            }
        }
    }

    fn record_exit(&mut self) {
        let stable = self
            .started_at
            .map(|started_at| started_at.elapsed() >= STABLE_PROCESS_AGE)
            .unwrap_or(false);

        self.child = None;
        self.started_at = None;
        self.schedule_restart(stable);
    }

    fn schedule_restart(&mut self, was_stable: bool) {
        if was_stable {
            self.restart_delay = INITIAL_RESTART_DELAY;
        } else {
            self.restart_delay = self
                .restart_delay
                .checked_mul(2)
                .unwrap_or(MAX_RESTART_DELAY)
                .min(MAX_RESTART_DELAY);
        }
        self.next_restart_at = Instant::now() + self.restart_delay;
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_child(directory: &Path, executable_name: &str) -> Result<Child> {
    let executable = directory.join(executable_name);
    if !executable.is_file() {
        anyhow::bail!("missing {}", executable.display());
    }

    Command::new(&executable)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .with_context(|| format!("failed to start {}", executable.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_delay_is_bounded() {
        let mut process = ManagedProcess::new("missing.exe", PathBuf::from("."));
        for _ in 0..20 {
            process.schedule_restart(false);
        }
        assert_eq!(process.restart_delay, MAX_RESTART_DELAY);
    }

    #[test]
    fn stable_process_resets_backoff() {
        let mut process = ManagedProcess::new("missing.exe", PathBuf::from("."));
        process.restart_delay = MAX_RESTART_DELAY;
        process.schedule_restart(true);
        assert_eq!(process.restart_delay, INITIAL_RESTART_DELAY);
    }
}
