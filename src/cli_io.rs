use pasteforward::error::{Error, Result};
use std::io::{self, IsTerminal, Write};

pub fn prompt_yes_no(prompt: &str, default_yes: bool) -> Result<bool> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    let suffix = if default_yes { "[Y/n]" } else { "[y/N]" };
    print!("{prompt} {suffix} ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Ok(default_yes);
    }
    Ok(matches!(trimmed.as_str(), "y" | "yes"))
}

pub fn required_value<'a>(args: &'a [String], index: usize, flag: &str) -> Result<&'a str> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| Error::Usage(format!("missing value for {flag}")))
}

pub fn parse_assignment(value: &str) -> Result<(String, String)> {
    let (key, val) = value
        .split_once('=')
        .ok_or_else(|| Error::Usage(format!("expected KEY=VALUE, got {value}")))?;
    if key.is_empty() {
        return Err(Error::Usage(format!("expected KEY=VALUE, got {value}")));
    }
    Ok((key.to_string(), val.to_string()))
}

pub fn interactive_init_args() -> Result<Vec<String>> {
    if !io::stdin().is_terminal() {
        return Err(Error::Usage(
            "interactive init requires a terminal; use: pasteforward init <dest> --host <ssh-host>"
                .to_string(),
        ));
    }
    println!("PasteForward setup");
    let destination = prompt_required("Destination name")?;
    let host = prompt_required("SSH destination")?;
    Ok(vec![destination, "--host".to_string(), host])
}

fn prompt_required(prompt: &str) -> Result<String> {
    print!("{prompt}: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let value = input.trim().to_string();
    if value.is_empty() {
        return Err(Error::Usage(format!("{prompt} cannot be blank")));
    }
    Ok(value)
}

pub fn usage() -> &'static str {
    r#"Usage:
  pasteforward init <dest> --host <ssh-host> [options]
  pasteforward doctor [dest]
  pasteforward status [dest]
  pasteforward delete <dest> [--purge]
  pasteforward list
  pasteforward history [dest]
  pasteforward cleanup [dest]
  pasteforward test <dest>
  pasteforward install-service
  pasteforward uninstall-service
  pasteforward daemon
  pasteforward --version

Init options:
  --remote-mode auto|macos-pasteboard|linux-wayland|linux-x11
  --remote-env KEY=VALUE
  --remote-dir DIR
  --install-service
  --no-install-service
  --image-history
  --no-image-history
  --yes
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_interactive_prompt_does_not_default_to_yes() {
        if !io::stdin().is_terminal() {
            assert!(!prompt_yes_no("Install?", true).unwrap());
        }
    }
}
