use crate::error::{Error, Result};
use std::process::{Command, Stdio};

#[derive(Debug)]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
}

pub fn run(program: &str, args: &[String], input: Option<&[u8]>) -> Result<CommandOutput> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }

    let mut child = cmd.spawn()?;
    if let Some(bytes) = input {
        use std::io::Write;
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| Error::UnsupportedPlatform("failed to open child stdin".to_string()))?;
        stdin.write_all(bytes)?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            program: program.to_string(),
            args: args.to_vec(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    Ok(CommandOutput {
        stdout: output.stdout,
    })
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
}
