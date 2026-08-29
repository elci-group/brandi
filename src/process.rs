//! Bounded subprocess execution shared by every external integration.

use std::io::{self, Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const MAX_SUBPROCESS_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_SUBPROCESS_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug)]
pub struct BoundedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub truncated: bool,
}

pub fn run_bounded(
    command: &mut Command,
    timeout: Duration,
    max_output_bytes: usize,
) -> io::Result<BoundedOutput> {
    run_bounded_with_input(command, None, timeout, max_output_bytes)
}

pub fn run_bounded_with_input(
    command: &mut Command,
    input: Option<&[u8]>,
    timeout: Duration,
    max_output_bytes: usize,
) -> io::Result<BoundedOutput> {
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;

    if let Some(bytes) = input {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("subprocess stdin was not captured"))?;
        stdin.write_all(bytes)?;
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("subprocess stdout was not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("subprocess stderr was not captured"))?;
    let stdout_reader = thread::spawn(move || read_and_drain(stdout, max_output_bytes));
    let stderr_reader = thread::spawn(move || read_and_drain(stderr, max_output_bytes));

    let deadline = Instant::now() + timeout;
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            break (child.wait()?, true);
        }
        thread::sleep(Duration::from_millis(25));
    };

    let (stdout, stdout_truncated) = stdout_reader
        .join()
        .map_err(|_| io::Error::other("subprocess stdout reader panicked"))??;
    let (stderr, stderr_truncated) = stderr_reader
        .join()
        .map_err(|_| io::Error::other("subprocess stderr reader panicked"))??;

    Ok(BoundedOutput {
        status,
        stdout,
        stderr,
        timed_out,
        truncated: stdout_truncated || stderr_truncated,
    })
}

fn read_and_drain(mut reader: impl Read, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut kept = Vec::with_capacity(limit.min(64 * 1024));
    let mut truncated = false;
    let mut chunk = [0u8; 8192];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(kept.len());
        let take = remaining.min(count);
        kept.extend_from_slice(&chunk[..take]);
        truncated |= take < count;
    }
    Ok((kept, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_output_without_deadlocking() {
        let output = run_bounded(
            Command::new("sh").args(["-c", "yes x | head -c 200000"]),
            Duration::from_secs(2),
            1024,
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 1024);
        assert!(output.truncated);
    }

    #[test]
    fn terminates_hung_processes() {
        let output = run_bounded(
            Command::new("sh").args(["-c", "sleep 5"]),
            Duration::from_millis(50),
            1024,
        )
        .unwrap();
        assert!(output.timed_out);
    }
}
