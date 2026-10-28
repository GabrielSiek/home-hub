//! The server as a Windows service: it starts with the laptop, before anyone signs in, and comes
//! back after a crash or a Windows update. `tvl-server install` registers it (run it from an
//! administrator console on the server laptop), `tvl-server uninstall` removes it (the data stays).
//!
//! It runs under its own virtual account, `NT SERVICE\tvl-server`, which can read and write only
//! the server's data folder (the household's keys, the home CA's key, the devices' local keys are
//! there). Services never get Windows' "allow access?" prompt, so `install` adds the firewall rule
//! itself, for home networks (the Private profile) only.

use std::ffi::OsString;
use std::path::Path;

/// The service's name, also its virtual account's.
pub const NAME: &str = "tvl-server";
const DISPLAY: &str = "TV Launcher home server";
const DESCRIPTION: &str = "The house's TV Launcher: smart home, timers and alarms, calendar, the companion's pages, and the relay between the TVs, phones and tablets.";
const FIREWALL_RULE: &str = "TV Launcher home server";

/// `netsh` arguments for the firewall rule: everything the exe listens on (the companion's ports,
/// the TV PCs' probe, the smart-home devices' broadcasts), from home networks only.
pub fn firewall_args(exe: &Path) -> Vec<String> {
    vec![
        "advfirewall".into(),
        "firewall".into(),
        "add".into(),
        "rule".into(),
        format!("name={FIREWALL_RULE}"),
        "dir=in".into(),
        "action=allow".into(),
        format!("program={}", exe.display()),
        "enable=yes".into(),
        "profile=private".into(),
    ]
}

/// `icacls` arguments that leave the data folder to the service's account, the administrators and
/// the system: nobody else signed in to the laptop can read the keys in it.
pub fn acl_args(dir: &Path) -> Vec<String> {
    vec![
        dir.display().to_string(),
        "/inheritance:r".into(),
        "/grant:r".into(),
        format!("NT SERVICE\\{NAME}:(OI)(CI)M"),
        // Administrators and SYSTEM, by well-known SID (their names are translated on non-English
        // Windows).
        "*S-1-5-32-544:(OI)(CI)F".into(),
        "*S-1-5-18:(OI)(CI)F".into(),
    ]
}

fn run(program: &str, args: &[String]) -> Result<(), String> {
    let out = std::process::Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    }
}

#[cfg(windows)]
pub fn install(data_dir: &Path) -> Result<(), String> {
    use std::time::Duration;
    use windows_service::service::{
        ServiceAccess, ServiceAction, ServiceActionType, ServiceErrorControl, ServiceFailureActions, ServiceFailureResetPeriod,
        ServiceInfo, ServiceStartType, ServiceType,
    };
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)
        .map_err(|e| format!("Run this from an administrator console ({e})"))?;
    let info = ServiceInfo {
        name: OsString::from(NAME),
        display_name: OsString::from(DISPLAY),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe.clone(),
        launch_arguments: vec![OsString::from("service")],
        dependencies: vec![],
        account_name: Some(OsString::from(format!("NT SERVICE\\{NAME}"))),
        account_password: None,
    };
    let access = ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::QUERY_STATUS;
    let service = manager.create_service(&info, access).map_err(|e| format!("can't register the service: {e}"))?;
    service.set_description(DESCRIPTION).map_err(|e| e.to_string())?;
    // After the network and the rest of Windows are up.
    service.set_delayed_auto_start(true).map_err(|e| e.to_string())?;
    let restart = |secs| ServiceAction { action_type: ServiceActionType::Restart, delay: Duration::from_secs(secs) };
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(24 * 3600)),
            reboot_msg: None,
            command: None,
            actions: Some(vec![restart(5), restart(30), restart(60)]),
        })
        .map_err(|e| e.to_string())?;

    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    run("icacls", &acl_args(data_dir))?;
    // An older rule of the same name goes first, so reinstalling doesn't stack them.
    let _ = run("netsh", &["advfirewall".into(), "firewall".into(), "delete".into(), "rule".into(), format!("name={FIREWALL_RULE}")]);
    run("netsh", &firewall_args(&exe))?;
    service.start::<&str>(&[]).map_err(|e| format!("registered, but it didn't start: {e}"))?;
    Ok(())
}

#[cfg(windows)]
pub fn uninstall() -> Result<(), String> {
    use windows_service::service::{ServiceAccess, ServiceState};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| format!("Run this from an administrator console ({e})"))?;
    let service = manager
        .open_service(NAME, ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS)
        .map_err(|e| format!("no {NAME} service: {e}"))?;
    if service.query_status().map(|s| s.current_state != ServiceState::Stopped).unwrap_or(false) {
        let _ = service.stop();
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    service.delete().map_err(|e| e.to_string())?;
    let _ = run("netsh", &["advfirewall".into(), "firewall".into(), "delete".into(), "rule".into(), format!("name={FIREWALL_RULE}")]);
    Ok(())
}

/// What Windows runs (`tvl-server service`): hands control to the service dispatcher, which calls
/// `service_main` on its own thread. Returns when the service has stopped.
#[cfg(windows)]
pub fn run_as_service() -> Result<(), String> {
    windows_service::service_dispatcher::start(NAME, ffi_service_main).map_err(|e| format!("not started by Windows: {e}"))
}

#[cfg(windows)]
windows_service::define_windows_service!(ffi_service_main, service_main);

#[cfg(windows)]
fn service_main(_args: Vec<OsString>) {
    use std::time::Duration;
    use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType};
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};

    let config = match crate::Config::service().with_file() {
        Ok(c) => c,
        Err(e) => return eprintln!("server.toml: {e}"),
    };
    let _log = tvl_kit::log::init(Some(&config.logs()), "server");
    let stop = tokio_util::sync::CancellationToken::new();
    let on_stop = stop.clone();
    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Preshutdown => {
            on_stop.cancel();
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let status = match service_control_handler::register(NAME, handler) {
        Ok(s) => s,
        Err(e) => return tracing::error!("can't take the service's controls: {e}"),
    };
    let report = |state, accept, wait| {
        let _ = status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: wait,
            process_id: None,
        });
    };
    report(ServiceState::StartPending, ServiceControlAccept::empty(), Duration::from_secs(10));
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("no runtime: {e}");
            return report(ServiceState::Stopped, ServiceControlAccept::empty(), Duration::ZERO);
        }
    };
    rt.block_on(async {
        match crate::start(config).await {
            Ok(running) => {
                report(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::PRESHUTDOWN, Duration::ZERO);
                tracing::info!("service running on port {}", running.http.port());
                stop.cancelled().await;
                report(ServiceState::StopPending, ServiceControlAccept::empty(), Duration::from_secs(5));
                running.stop();
                // The stores save as they change; give the sockets a moment to close.
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(e) => tracing::error!("the server didn't start: {e}"),
        }
    });
    report(ServiceState::Stopped, ServiceControlAccept::empty(), Duration::ZERO);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_firewall_rule_is_for_this_exe_on_home_networks_only() {
        let args = firewall_args(Path::new(r"C:\Program Files\TVLauncher\tvl-server.exe"));
        assert!(args.contains(&r"program=C:\Program Files\TVLauncher\tvl-server.exe".to_string()));
        assert!(args.contains(&"profile=private".to_string()));
        assert!(args.contains(&"dir=in".to_string()));
    }

    #[test]
    fn the_data_folder_is_left_to_the_service_admins_and_system() {
        let args = acl_args(Path::new(r"C:\ProgramData\TVLauncher\server"));
        assert_eq!(args[0], r"C:\ProgramData\TVLauncher\server");
        assert!(args.contains(&"/inheritance:r".to_string()), "nothing inherited from ProgramData's own rules");
        assert!(args.contains(&r"NT SERVICE\tvl-server:(OI)(CI)M".to_string()));
        assert_eq!(args.len(), 6);
    }
}
