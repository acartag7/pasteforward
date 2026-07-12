use pasteforward::config::{load_config, save_config};
use pasteforward::daemon::{cleanup_expired, run_daemon, sync_one_without_history};
use pasteforward::doctor::{doctor_destination, local_doctor_problem};
use pasteforward::error::{Error, Result};
use pasteforward::history::{HistoryEvent, purge_destination_history, read_history};
use pasteforward::remote::read_clipboard_command;
use pasteforward::service::{
    ServiceStatus, install_service, restart_service_if_installed, service_running, service_status,
    uninstall_service,
};
use pasteforward::state::{process_alive, read_pid};
use pasteforward::{clipboard, command};
use std::any::Any;
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
        "test" => cmd_test(args),
        "install-service" => cmd_install_service(args),
        "uninstall-service" => cmd_uninstall_service(args),
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

fn cmd_install_service(args: Vec<String>) -> Result<()> {
    if !args.is_empty() {
        return Err(Error::Usage(
            "usage: pasteforward install-service".to_string(),
        ));
    }
    let config = load_config()?;
    if config.destinations.is_empty() {
        return Err(Error::DoctorFailed(
            "configure at least one destination before installing the service".to_string(),
        ));
    }
    if let Some(problem) = local_doctor_problem() {
        return Err(Error::DoctorFailed(format!(
            "service not installed because local doctor failed: {problem}"
        )));
    }
    for (name, dest) in config.destinations.iter().filter(|(_, dest)| dest.enabled) {
        let report = doctor_destination(&config, name, dest);
        print_doctor(&report);
        if !report.ok() {
            return Err(Error::DoctorFailed(format!(
                "service not installed because doctor failed for {name}"
            )));
        }
    }
    install_service()?;
    println!("service installed and running");
    Ok(())
}

fn cmd_uninstall_service(args: Vec<String>) -> Result<()> {
    if !args.is_empty() {
        return Err(Error::Usage(
            "usage: pasteforward uninstall-service".to_string(),
        ));
    }
    uninstall_service()?;
    println!("service uninstalled; destinations and history were kept");
    Ok(())
}

fn cmd_init(args: Vec<String>) -> Result<()> {
    cli_setup::cmd_init(args)
}

fn cmd_doctor(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err(Error::Usage(
            "usage: pasteforward doctor [dest]".to_string(),
        ));
    }
    let config = load_config()?;
    let mut failed = false;
    if let Some(problem) = local_doctor_problem() {
        println!("local clipboard: FAIL - {problem}");
        failed = true;
    } else {
        println!("local clipboard: ok");
    }

    if args.is_empty() {
        for (name, dest) in &config.destinations {
            let report = doctor_destination(&config, name, dest);
            failed |= !report.ok();
            print_doctor(&report);
        }
        return if failed {
            Err(Error::DoctorFailed("doctor failed".to_string()))
        } else {
            Ok(())
        };
    }

    let name = &args[0];
    let dest = config
        .destinations
        .get(name)
        .ok_or_else(|| Error::MissingDestination(name.clone()))?;
    let report = doctor_destination(&config, name, dest);
    print_doctor(&report);
    if report.ok() && !failed {
        Ok(())
    } else {
        Err(Error::DoctorFailed(format!("doctor failed for {name}")))
    }
}

fn cmd_test(args: Vec<String>) -> Result<()> {
    if args.len() != 1 {
        return Err(Error::Usage("usage: pasteforward test <dest>".to_string()));
    }
    let config = load_config()?;
    let name = &args[0];
    let dest = config
        .destinations
        .get(name)
        .ok_or_else(|| Error::MissingDestination(name.clone()))?;
    let backend = clipboard::detect_local_backend()?;
    let image = clipboard::read_image(&backend)?.ok_or_else(|| {
        Error::DoctorFailed("local clipboard does not contain a PNG image".to_string())
    })?;

    println!("test will replace the remote clipboard for {name}");
    let (remote_path, mode) =
        sync_one_without_history(&config, name, dest, &image.bytes, &image.sha256)?;
    let read_command = read_clipboard_command(dest, &mode)?;
    let readback = command::ssh(&dest.host, &read_command, None);
    let cleanup = format!("rm -f {}", command::shell_quote(&remote_path));
    let cleanup_result = command::ssh(&dest.host, &cleanup, None);
    let remote = readback?.stdout;
    cleanup_result?;
    let remote_sha = clipboard::sha256_hex(&remote);
    if remote_sha != image.sha256 {
        return Err(Error::DoctorFailed(format!(
            "clipboard hash mismatch for {name}: local={} remote={remote_sha}",
            image.sha256
        )));
    }
    println!(
        "test: ok destination={name} mode={} bytes={} sha256={}",
        mode.as_str(),
        remote.len(),
        remote_sha
    );
    Ok(())
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
        ServiceStatus::Installed => {
            println!("service: installed");
            println!(
                "service running: {}",
                if service_running() { "yes" } else { "no" }
            );
        }
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
        println!("{}", history_line(&event));
    }
    Ok(())
}

fn history_line(event: &HistoryEvent) -> String {
    format!(
        "{} {} {} bytes {} {}",
        event.unix_ms, event.destination, event.bytes, event.sha256, event.remote_path
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_broken_pipe_print_panic() {
        let message = "failed printing to stdout: Broken pipe (os error 32)".to_string();
        assert!(panic_payload_is_broken_pipe(&message));
        assert!(!panic_payload_is_broken_pipe(&"other panic"));
    }

    #[test]
    fn history_lines_keep_the_sha_in_the_fifth_field() {
        let event = HistoryEvent {
            schema_version: 1,
            unix_ms: 42,
            destination: "limaone".to_string(),
            host: "lima-pasteforward-linux".to_string(),
            sha256: "abc123".to_string(),
            bytes: 67,
            remote_path: "/tmp/pasteforward/limaone.png".to_string(),
            remote_mode: pasteforward::config::RemoteMode::LinuxX11,
            image_history_path: None,
        };

        assert_eq!(
            history_line(&event).split_whitespace().collect::<Vec<_>>(),
            vec![
                "42",
                "limaone",
                "67",
                "bytes",
                "abc123",
                "/tmp/pasteforward/limaone.png",
            ]
        );
    }
}
mod cli_io;
mod cli_output;
mod cli_setup;

use cli_io::usage;
use cli_output::{print_destination_line, print_doctor};
