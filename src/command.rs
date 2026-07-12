use crate::error::{Error, Result};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const MAX_COMMAND_OUTPUT_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_COMMAND_ERROR_BYTES: usize = 64 * 1024;
pub const MAX_COMMAND_INPUT_BYTES: usize = 25 * 1024 * 1024;
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
}

pub fn run(program: &str, args: &[String], input: Option<&[u8]>) -> Result<CommandOutput> {
    run_with_timeout(program, args, input, COMMAND_TIMEOUT)
}

fn run_with_timeout(
    program: &str,
    args: &[String],
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<CommandOutput> {
    if input.is_some_and(|bytes| bytes.len() > MAX_COMMAND_INPUT_BYTES) {
        return Err(Error::LimitExceeded(format!(
            "command input exceeds {} byte limit",
            MAX_COMMAND_INPUT_BYTES
        )));
    }
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd.spawn()?;
    let child_pid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::UnsupportedPlatform("failed to capture child stdout".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::UnsupportedPlatform("failed to capture child stderr".to_string()))?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout, MAX_COMMAND_OUTPUT_BYTES));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAX_COMMAND_ERROR_BYTES));
    let mut stdin_writer = input.map(|bytes| {
        let mut stdin = child
            .stdin
            .take()
            .expect("stdin is piped when input exists");
        let bytes = bytes.to_vec();
        thread::spawn(move || stdin.write_all(&bytes))
    });

    let started = Instant::now();
    let mut child_status = None;
    let status = loop {
        if child_status.is_none() {
            child_status = child.try_wait()?;
        }
        let output_complete = stdout_reader.is_finished() && stderr_reader.is_finished();
        let input_complete = stdin_writer
            .as_ref()
            .is_none_or(thread::JoinHandle::is_finished);
        if output_complete && input_complete {
            if let Some(status) = child_status {
                break status;
            }
        }
        if started.elapsed() >= timeout {
            kill_child_tree(&mut child, child_pid);
            let _ = child.wait();
            if let Some(writer) = stdin_writer.take() {
                if writer.is_finished() {
                    let _ = writer.join();
                }
            }
            if stdout_reader.is_finished() {
                let _ = stdout_reader.join();
            }
            if stderr_reader.is_finished() {
                let _ = stderr_reader.join();
            }
            return Err(Error::CommandTimedOut {
                program: program.to_string(),
                seconds: timeout.as_secs(),
            });
        }
        thread::sleep(Duration::from_millis(25));
    };
    let stdin_result =
        if let Some(writer) = stdin_writer.take() {
            Some(writer.join().map_err(|_| {
                Error::UnsupportedPlatform("command input writer panicked".to_string())
            })?)
        } else {
            None
        };
    let stdout = join_reader(stdout_reader)?;
    let stderr = join_reader(stderr_reader)?;
    if stdout.len() > MAX_COMMAND_OUTPUT_BYTES {
        return Err(Error::LimitExceeded(format!(
            "command output exceeds {} byte limit: {program}",
            MAX_COMMAND_OUTPUT_BYTES
        )));
    }
    if stderr.len() > MAX_COMMAND_ERROR_BYTES {
        return Err(Error::LimitExceeded(format!(
            "command error output exceeds {} byte limit: {program}",
            MAX_COMMAND_ERROR_BYTES
        )));
    }
    if !status.success() {
        return Err(Error::CommandFailed {
            program: program.to_string(),
            args: args.to_vec(),
            code: status.code(),
            stderr: String::from_utf8_lossy(&stderr).to_string(),
        });
    }
    if let Some(result) = stdin_result {
        result?;
    }

    Ok(CommandOutput { stdout })
}

fn kill_child_tree(child: &mut std::process::Child, child_pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child_pid as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

fn read_bounded(mut reader: impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn join_reader(handle: thread::JoinHandle<std::io::Result<Vec<u8>>>) -> Result<Vec<u8>> {
    handle
        .join()
        .map_err(|_| Error::UnsupportedPlatform("command output reader panicked".to_string()))?
        .map_err(Error::Io)
}

pub fn run_ok(program: &str, args: &[String], input: Option<&[u8]>) -> bool {
    run(program, args, input).is_ok()
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn javascript_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn ssh(host: &str, remote_command: &str, input: Option<&[u8]>) -> Result<CommandOutput> {
    if host.is_empty() || host.starts_with('-') || host.chars().any(char::is_control) {
        return Err(Error::InvalidDestination(
            "SSH host must be non-empty, must not begin with '-', and must not contain control characters"
                .to_string(),
        ));
    }
    let args = vec![host.to_string(), remote_command.to_string()];
    run("ssh", &args, input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_single_quotes_for_shell() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn quotes_applescript_strings() {
        assert_eq!(applescript_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn quotes_javascript_strings() {
        assert_eq!(javascript_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }

    #[cfg(unix)]
    #[test]
    fn timeout_applies_while_child_stdin_is_blocked() {
        let input = vec![0_u8; 1024 * 1024];
        let result = run_with_timeout(
            "sh",
            &["-c".to_string(), "sleep 5".to_string()],
            Some(&input),
            Duration::from_millis(100),
        );
        assert!(matches!(result, Err(Error::CommandTimedOut { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_applies_to_descendants_holding_output_pipes() {
        let started = Instant::now();
        let result = run_with_timeout(
            "sh",
            &["-c".to_string(), "sleep 5 &".to_string()],
            None,
            Duration::from_millis(100),
        );
        assert!(matches!(result, Err(Error::CommandTimedOut { .. })));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
