use crate::cli_io::{interactive_init_args, parse_assignment, prompt_yes_no, required_value};
use crate::cli_output::print_doctor;
use pasteforward::config::{
    DestinationConfig, RemoteMode, config_path, load_config, remove_config, save_config,
};
use pasteforward::doctor::{doctor_destination, local_doctor_problem, prepare_remote_directory};
use pasteforward::error::{Error, Result};
use pasteforward::service::install_service_with_rollback_precondition;
use pasteforward::validation::{validate_config, validate_destination_name, validate_remote_env};
use std::collections::BTreeMap;

pub fn cmd_init(args: Vec<String>) -> Result<()> {
    let args = if args.is_empty() {
        interactive_init_args()?
    } else {
        args
    };
    let dest_name = args[0].clone();
    validate_destination_name(&dest_name)?;
    let options = parse_options(&args)?;
    let mut config = load_config()?;
    let previous_config = config.clone();
    let had_config = config_path()?.exists();
    if let Some(enabled) = options.image_history {
        config.history.image = enabled;
    }
    config.destinations.insert(
        dest_name.clone(),
        DestinationConfig {
            host: options
                .host
                .ok_or_else(|| Error::Usage("--host is required".to_string()))?,
            enabled: true,
            remote_mode: options.remote_mode,
            remote_env: options.remote_env,
            remote_dir: options.remote_dir,
        },
    );
    validate_config(&config)?;

    if let Some(problem) = local_doctor_problem() {
        println!("local clipboard: FAIL - {problem}");
        return Err(Error::DoctorFailed(
            "init did not write config or install service because local doctor failed".to_string(),
        ));
    }
    println!("local clipboard: ok");
    let dest = config.destinations.get(&dest_name).expect("inserted above");
    let report = doctor_destination(&config, &dest_name, dest);
    print_doctor(&report);
    if !report.ok() {
        return Err(Error::DoctorFailed(format!(
            "init did not write config or install service because doctor failed for {dest_name}"
        )));
    }
    prepare_remote_directory(&config, dest)?;
    save_config(&config)?;

    let should_install = match options.install {
        Some(value) => value,
        None if options.yes => true,
        None => prompt_yes_no("Install or restart the background service now?", true)?,
    };
    if should_install {
        let mut rollback_precondition_ran = false;
        if let Err(error) = install_service_with_rollback_precondition(|| {
            rollback_precondition_ran = true;
            restore_config_with_retry(|| restore_previous_config(had_config, &previous_config))
        }) {
            if !rollback_precondition_ran {
                if let Err(config_restore) = restore_previous_config(had_config, &previous_config) {
                    return combine_service_and_config_restore_error(error, config_restore);
                }
            }
            return Err(error);
        }
        println!("service installed and running");
    } else {
        println!("service install skipped");
    }
    Ok(())
}

fn restore_previous_config(
    had_config: bool,
    previous_config: &pasteforward::config::AppConfig,
) -> Result<()> {
    if had_config {
        save_config(previous_config)
    } else {
        remove_config()
    }
}

fn restore_config_with_retry(mut restore_config: impl FnMut() -> Result<()>) -> Result<()> {
    match restore_config() {
        Ok(()) => Ok(()),
        Err(first) => restore_config().map_err(|second| {
            Error::DoctorFailed(format!(
                "configuration restoration failed twice ({first}; {second})"
            ))
        }),
    }
}

fn combine_service_and_config_restore_error(
    service_error: Error,
    config_restore: Error,
) -> Result<()> {
    Err(Error::DoctorFailed(format!(
        "service installation failed ({service_error}) and configuration restoration failed ({config_restore})"
    )))
}

struct InitOptions {
    host: Option<String>,
    remote_mode: RemoteMode,
    remote_dir: Option<String>,
    remote_env: BTreeMap<String, String>,
    install: Option<bool>,
    yes: bool,
    image_history: Option<bool>,
}

fn parse_options(args: &[String]) -> Result<InitOptions> {
    let mut options = InitOptions {
        host: None,
        remote_mode: RemoteMode::Auto,
        remote_dir: None,
        remote_env: BTreeMap::new(),
        install: None,
        yes: false,
        image_history: None,
    };
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--host" => {
                i += 1;
                options.host = Some(required_value(args, i, "--host")?.to_string());
            }
            "--remote-mode" => {
                i += 1;
                options.remote_mode = RemoteMode::parse(required_value(args, i, "--remote-mode")?)?;
            }
            "--remote-dir" => {
                i += 1;
                options.remote_dir = Some(required_value(args, i, "--remote-dir")?.to_string());
            }
            "--remote-env" => {
                i += 1;
                let (key, value) = parse_assignment(required_value(args, i, "--remote-env")?)?;
                validate_remote_env(&key, &value)?;
                options.remote_env.insert(key, value);
            }
            "--install-service" => options.install = Some(true),
            "--no-install-service" => options.install = Some(false),
            "--yes" | "-y" => options.yes = true,
            "--image-history" => options.image_history = Some(true),
            "--no-image-history" => options.image_history = Some(false),
            other => return Err(Error::Usage(format!("unknown init option: {other}"))),
        }
        i += 1;
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn config_restoration_retries_inside_the_rollback_precondition() {
        let restore_calls = Cell::new(0);
        restore_config_with_retry(|| {
            restore_calls.set(restore_calls.get() + 1);
            if restore_calls.get() == 1 {
                Err(Error::DoctorFailed("first restore failed".to_string()))
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert_eq!(restore_calls.get(), 2);
    }

    #[test]
    fn config_restoration_retry_preserves_both_failures() {
        let restore_calls = Cell::new(0);
        let result = restore_config_with_retry(|| {
            restore_calls.set(restore_calls.get() + 1);
            Err(Error::DoctorFailed(format!(
                "restore attempt {} failed",
                restore_calls.get()
            )))
        });
        assert_eq!(restore_calls.get(), 2);
        assert!(
            matches!(result, Err(Error::DoctorFailed(message)) if message.contains("restore attempt 1 failed") && message.contains("restore attempt 2 failed"))
        );
    }

    #[test]
    fn pre_activation_restore_failure_retains_the_service_error() {
        let result = combine_service_and_config_restore_error(
            Error::DoctorFailed("service activation failed".to_string()),
            Error::DoctorFailed("config restore failed".to_string()),
        );
        assert!(
            matches!(result, Err(Error::DoctorFailed(message)) if message.contains("service activation failed") && message.contains("config restore failed"))
        );
    }
}
