use pasteforward::config::{AppConfig, DestinationConfig, RemoteMode};
use pasteforward::doctor::DestinationDoctor;

pub fn print_doctor(report: &DestinationDoctor) {
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

pub fn print_destination_line(name: &str, dest: &DestinationConfig, config: &AppConfig) {
    println!(
        "{} host={} enabled={} mode={} remote_dir={}",
        name,
        dest.host,
        dest.enabled,
        dest.remote_mode.as_str(),
        config.destination_remote_dir(dest)
    );
}
