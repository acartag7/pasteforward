use pasteforward::config::{
    AppConfig, DestinationConfig, RemoteMode, load_config, save_config, validate_destination_name,
};
use pasteforward::daemon::{cleanup_expired, run_daemon};
use pasteforward::doctor::{doctor_destination, local_doctor_problem};
use pasteforward::error::{Error, Result};
use pasteforward::history::{purge_destination_history, read_history};
use pasteforward::service::{
    ServiceStatus, install_service, restart_service_if_installed, service_status, uninstall_service,
};
use pasteforward::state::{process_alive, read_pid};
use std::any::Any;
use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::panic;

fn main() {
    install_broken_pipe_panic_hook();
    match panic::catch_unwind(run_cli) {
        Ok(Ok(())) => {}
        Ok(Err(err)) => exit_with_error(err),
        Err(payload) if panic_payload_is_broken_pipe(payload.as_ref()) => std::process::exit(0),
        Err(payload) => panic::resume_unwind(payload),
    }
}

fn exit_with_error(err: Error) -> ! {
    eprintln!("pasteforward: {err}");
    if matches!(err, Error::Usage(_)) {
        eprintln!();
        eprintln!("{}", usage());
    }
    std::process::exit(1);
}

fn install_broken_pipe_panic_hook() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if !panic_payload_is_broken_pipe(info.payload()) {
            default_hook(info);
        }
    }));
}

fn panic_payload_is_broken_pipe(payload: &(dyn Any + Send)) -> bool {
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied());
    message.is_some_and(|value| {
        value.contains("failed printing to stdout") && value.contains("Broken pipe")
    })
}

fn run_cli() -> Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Err(Error::Usage("missing command".to_string()));
    }
    let command = args.remove(0);
    match command.as_str() {
        "init" => cmd_init(args),
        "doctor" => cmd_doctor(args),
        "status" => cmd_status(args),
        "delete" => cmd_delete(args),
        "list" => cmd_list(args),
        "history" => cmd_history(args),
        "cleanup" => cmd_cleanup(args),
        "install-service" => cmd_init(args),
        "uninstall-service" => cmd_delete(args),
        "daemon" => run_daemon(),
        "help" | "-h" | "--help" => {
            println!("{}", usage());
            Ok(())
        }
        "version" | "-V" | "--version" => {
            println!("pasteforward {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(Error::Usage(format!("unknown command: {command}"))),
    }
}

fn cmd_init(args: Vec<String>) -> Result<()> {
    if args.is_empty() {
        return Err(Error::Usage(
            "usage: pasteforward init <dest> --host <ssh-host>".to_string(),
        ));
    }
    let dest_name = args[0].clone();
    validate_destination_name(&dest_name)?;

    let mut host = None;
    let mut remote_mode = RemoteMode::Auto;
    let mut remote_dir = None;
    let mut remote_env = BTreeMap::new();
    let mut install = None;
    let mut yes = false;
    let mut image_history = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--host" => {
                i += 1;
                host = Some(required_value(&args, i, "--host")?.to_string());
            }
            "--remote-mode" => {
                i += 1;
                remote_mode = RemoteMode::parse(required_value(&args, i, "--remote-mode")?)?;
            }
            "--remote-dir" => {
                i += 1;
                remote_dir = Some(required_value(&args, i, "--remote-dir")?.to_string());
            }
            "--remote-env" => {
                i += 1;
                let value = required_value(&args, i, "--remote-env")?;
                let (key, val) = parse_assignment(value)?;
                remote_env.insert(key, val);
            }
            "--install-service" => install = Some(true),
            "--no-install-service" => install = Some(false),
            "--yes" | "-y" => yes = true,
            "--image-history" => image_history = Some(true),
            "--no-image-history" => image_history = Some(false),
            other => return Err(Error::Usage(format!("unknown init option: {other}"))),
        }
        i += 1;
    }

    let host = host.ok_or_else(|| Error::Usage("--host is required".to_string()))?;
    let mut config = load_config()?;
    if let Some(enabled) = image_history {
        config.history.image = enabled;
    }
    config.destinations.insert(
        dest_name.clone(),
        DestinationConfig {
            host,
            enabled: true,
            remote_mode,
            remote_env,
            remote_dir,
        },
    );
    save_config(&config)?;

    if let Some(problem) = local_doctor_problem() {
        println!("local clipboard: FAIL - {problem}");
        println!("init wrote config but did not install service because local doctor failed");
        return Ok(());
    }
    println!("local clipboard: ok");

    let dest = config.destinations.get(&dest_name).unwrap();
    let report = doctor_destination(&config, &dest_name, dest);
    print_doctor(&report);
    if !report.ok() {
        println!("init wrote config but did not install service because doctor failed");
        return Ok(());
    }

    let should_install = match install {
        Some(value) => value,
        None if yes => true,
        None => prompt_yes_no("Install or restart the background service now?", true)?,
    };
    if should_install {
        install_service()?;
        println!("service installed and running");
    } else {
        println!("service install skipped");
    }

    Ok(())
}

fn cmd_doctor(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err(Error::Usage(
            "usage: pasteforward doctor [dest]".to_string(),
        ));
    }
    let config = load_config()?;
    if let Some(problem) = local_doctor_problem() {
        println!("local clipboard: FAIL - {problem}");
    } else {
        println!("local clipboard: ok");
    }

    if args.is_empty() {
        for (name, dest) in &config.destinations {
            print_doctor(&doctor_destination(&config, name, dest));
        }
        return Ok(());
    }

    let name = &args[0];
    let dest = config
        .destinations
        .get(name)
        .ok_or_else(|| Error::MissingDestination(name.clone()))?;
    let report = doctor_destination(&config, name, dest);
    print_doctor(&report);
    if report.ok() {
        Ok(())
    } else {
        Err(Error::DoctorFailed(format!("doctor failed for {name}")))
    }
}

fn cmd_status(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err(Error::Usage(
            "usage: pasteforward status [dest]".to_string(),
        ));
    }
    let config = load_config()?;
    println!("config: {}", pasteforward::config::config_path()?.display());
    println!(
        "history: {}",
        pasteforward::config::history_path()?.display()
    );
    println!("metadata history: {}", config.history.metadata);
    println!("image history: {}", config.history.image);
    println!("remote dir: {}", config.remote_dir);
    println!("ttl seconds: {}", config.retention.ttl_seconds);
    match service_status()? {
        ServiceStatus::Installed => println!("service: installed"),
        ServiceStatus::NotInstalled => println!("service: not installed"),
        ServiceStatus::Unknown(message) => println!("service: unknown - {message}"),
    }
    match read_pid()? {
        Some(pid) if process_alive(pid) => println!("daemon pid: {pid} (running)"),
        Some(pid) => println!("daemon pid: {pid} (stale)"),
        None => println!("daemon pid: not recorded"),
    }

    if args.is_empty() {
        println!("destinations: {}", config.destinations.len());
        for (name, dest) in &config.destinations {
            print_destination_line(name, dest, &config);
        }
        return Ok(());
    }

    let name = &args[0];
    let dest = config
        .destinations
        .get(name)
        .ok_or_else(|| Error::MissingDestination(name.clone()))?;
    print_destination_line(name, dest, &config);
    Ok(())
}

fn cmd_delete(args: Vec<String>) -> Result<()> {
    if args.is_empty() {
        return Err(Error::Usage(
            "usage: pasteforward delete <dest> [--purge]".to_string(),
        ));
    }
    let name = args[0].clone();
    let mut purge = false;
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "--purge" => purge = true,
            other => return Err(Error::Usage(format!("unknown delete option: {other}"))),
        }
    }
    let mut config = load_config()?;
    if config.destinations.remove(&name).is_none() {
        return Err(Error::MissingDestination(name));
    }
    save_config(&config)?;
    if purge {
        purge_destination_history(&args[0])?;
    }

    if config.destinations.is_empty() {
        uninstall_service()?;
        println!("removed destination and uninstalled service because no destinations remain");
    } else {
        restart_service_if_installed()?;
        println!("removed destination and reloaded service");
    }
    Ok(())
}

fn cmd_list(args: Vec<String>) -> Result<()> {
    if !args.is_empty() {
        return Err(Error::Usage("usage: pasteforward list".to_string()));
    }
    let config = load_config()?;
    if config.destinations.is_empty() {
        println!("no destinations configured");
        return Ok(());
    }
    for (name, dest) in &config.destinations {
        print_destination_line(name, dest, &config);
    }
    Ok(())
}

fn cmd_history(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err(Error::Usage(
            "usage: pasteforward history [dest]".to_string(),
        ));
    }
    let destination = args.first().map(String::as_str);
    let events = read_history(destination, 50)?;
    for event in events {
        println!(
            "{} {} {} bytes {} {}",
            event.unix_ms, event.destination, event.bytes, event.sha256, event.remote_path
        );
    }
    Ok(())
}

fn cmd_cleanup(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err(Error::Usage(
            "usage: pasteforward cleanup [dest]".to_string(),
        ));
    }
    let config = load_config()?;
    let destination = args.first().map(String::as_str);
    cleanup_expired(&config, destination)?;
    println!("cleanup complete");
    Ok(())
}

fn print_doctor(report: &pasteforward::doctor::DestinationDoctor) {
    println!("destination: {}", report.name);
    println!("  host: {}", report.host);
    println!("  enabled: {}", report.enabled);
    println!("  ssh: {}", if report.ssh_ok { "ok" } else { "fail" });
    println!(
        "  remote mode: {}",
        report
            .remote_mode
            .as_ref()
            .map(RemoteMode::as_str)
            .unwrap_or("unknown")
    );
    println!(
        "  remote clipboard: {}",
        if report.remote_clipboard_ok {
            "ok"
        } else {
            "fail"
        }
    );
    println!(
        "  remote dir: {}",
        if report.remote_dir_ok { "ok" } else { "fail" }
    );
    for problem in &report.problems {
        println!("  problem: {problem}");
    }
}

fn print_destination_line(name: &str, dest: &DestinationConfig, config: &AppConfig) {
    println!(
        "{} host={} enabled={} mode={} remote_dir={}",
        name,
        dest.host,
        dest.enabled,
        dest.remote_mode.as_str(),
        config.destination_remote_dir(dest)
    );
}

fn prompt_yes_no(prompt: &str, default_yes: bool) -> Result<bool> {
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

fn required_value<'a>(args: &'a [String], index: usize, flag: &str) -> Result<&'a str> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| Error::Usage(format!("missing value for {flag}")))
}

fn parse_assignment(value: &str) -> Result<(String, String)> {
    let (key, val) = value
        .split_once('=')
        .ok_or_else(|| Error::Usage(format!("expected KEY=VALUE, got {value}")))?;
    if key.is_empty() {
        return Err(Error::Usage(format!("expected KEY=VALUE, got {value}")));
    }
    Ok((key.to_string(), val.to_string()))
}

fn usage() -> &'static str {
    r#"Usage:
  pasteforward init <dest> --host <ssh-host> [options]
  pasteforward doctor [dest]
  pasteforward status [dest]
  pasteforward delete <dest> [--purge]
  pasteforward list
  pasteforward history [dest]
  pasteforward cleanup [dest]
  pasteforward install-service <dest> --host <ssh-host> [options]
  pasteforward uninstall-service <dest> [--purge]
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

    #[test]
    fn detects_broken_pipe_print_panic() {
        let message = "failed printing to stdout: Broken pipe (os error 32)".to_string();
        assert!(panic_payload_is_broken_pipe(&message));
        assert!(!panic_payload_is_broken_pipe(&"other panic"));
    }
}
