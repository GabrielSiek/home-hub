//! `tvl-server.exe`: the home server, run as a Windows service or in a console.

use std::process::ExitCode;

use tvl_server::admin::Admin;
use tvl_server::Config;

const USAGE: &str = "\
tvl-server: the TV Launcher home server

  tvl-server run [--dev] [--data <folder>] [--port <http port>] [--shadow] [--force-lan]
      Runs in this console until Ctrl+C. --dev uses the side-by-side setup (ports 47970-47972,
      data in %LOCALAPPDATA%\\TVLauncher\\server-dev); --shadow leaves the smart-home LAN,
      automations, ringing and calendar cards to the old launcher.
  tvl-server install / uninstall
      Registers (or removes) the Windows service that runs the server at boot: from an
      administrator console on the server laptop. The data folder stays on uninstall.
  tvl-server pin [--reset] [--dev] [--data <folder>]
      Shows (or makes anew) the admin PIN.
  tvl-server import [--from <old data folder>] [--env <.env file>] [--out <bundle.zip>]
                    [--dry-run] [--force] [--dev] [--data <folder>]
      Copies the old launcher's household data (smart home, rooms, tasks, Hub, timers, calendar,
      the home certificate authority, settings and keys) into the server's data folder, or into a
      bundle to carry to the server laptop (--out). The old folder (default
      %APPDATA%\\dev.tvlauncher.app) is only read. Files already there are kept unless --force.
  tvl-server import --bundle <bundle.zip> [--dry-run] [--force] [--dev] [--data <folder>]
      Unpacks a bundle made on the TV PC into the server's data folder.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("run") => run(rest),
        Some("pin") => pin(rest),
        Some("import") => import(rest),
        #[cfg(windows)]
        Some("install") => install(rest),
        #[cfg(windows)]
        Some("uninstall") => tvl_server::service::uninstall().map(|()| println!("Removed the service; the data folder stays.")),
        #[cfg(windows)]
        Some("service") => tvl_server::service::run_as_service(),
        Some("help" | "--help" | "-h") | None => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(format!("unknown command {other}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn config(args: &[String]) -> Result<Config, String> {
    let mut c = if flag(args, "--dev") { Config::dev() } else { Config::service() };
    if let Some(d) = option(args, "--data") {
        c.data_dir = d.into();
    }
    c = c.with_file().map_err(|e| format!("server.toml: {e}"))?;
    if let Some(p) = option(args, "--port") {
        let p: u16 = p.parse().map_err(|_| format!("not a port: {p}"))?;
        c.http_port = p;
        c.https_port = c.https_port.map(|_| p + 1);
        c.probe_port = c.probe_port.map(|_| p + 2);
    }
    c.shadow = flag(args, "--shadow");
    c.force_lan = flag(args, "--force-lan");
    Ok(c)
}

fn run(args: &[String]) -> Result<(), String> {
    let config = config(args)?;
    let _log = tvl_kit::log::init(Some(&config.logs()), "server");
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string())?;
    rt.block_on(async {
        let running = tvl_server::start(config).await.map_err(|e| e.to_string())?;
        let here = tvl_server::tls::hostname().unwrap_or_else(|| "this-pc".into());
        tracing::info!("open http://{here}:{} (or this PC's address) on a phone or tablet", running.http.port());
        if let Some(https) = running.https {
            tracing::info!("HTTPS on port {} once the device trusts /ca.crt", https.port());
        }
        if running.ctx.config.shadow {
            tracing::info!("shadow mode: the smart-home LAN, automations, ringing and calendar cards are left to the old launcher");
        }
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("stopping");
        running.stop();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        Ok(())
    })
}

fn import(args: &[String]) -> Result<(), String> {
    use tvl_server::import::{self, Outcome};
    let dry_run = flag(args, "--dry-run");
    let items = if let Some(zip) = option(args, "--bundle") {
        import::unbundle(std::path::Path::new(zip)).map_err(|e| e.to_string())?
    } else {
        let from = match option(args, "--from") {
            Some(f) => std::path::PathBuf::from(f),
            None => std::env::var_os("APPDATA").map(std::path::PathBuf::from).ok_or("say --from")?.join("dev.tvlauncher.app"),
        };
        println!("Reading {} (nothing there is changed)", from.display());
        let g = import::gather(&from, option(args, "--env").map(std::path::Path::new)).map_err(|e| e.to_string())?;
        for n in &g.notes {
            println!("  note: {n}");
        }
        if let Some(out) = option(args, "--out") {
            if dry_run {
                println!("Would pack {} files into {out}", g.items.len());
            } else {
                import::bundle(&g.items, std::path::Path::new(out)).map_err(|e| e.to_string())?;
                println!("Packed {} files into {out}: carry it to the server and run `tvl-server import --bundle {out}`", g.items.len());
                println!("It holds the household's keys and the home CA's key: delete it once imported.");
            }
            return Ok(());
        }
        g.items
    };
    let config = config(args)?;
    let target = &config.data_dir;
    println!("{} into {}", if dry_run { "Would write" } else { "Writing" }, target.display());
    let report = import::write_into(target, &items, flag(args, "--force"), dry_run).map_err(|e| e.to_string())?;
    for (name, outcome) in &report {
        let what = match outcome {
            Outcome::Written if dry_run => "would be written",
            Outcome::Written => "written",
            Outcome::Unchanged => "already there",
            Outcome::Kept => "kept (it's already there and different: --force replaces it)",
        };
        println!("  {name}: {what}");
    }
    if report.iter().any(|(n, o)| n.starts_with("companion-ca") && *o == Outcome::Kept) {
        println!("The server already made its own home CA: devices that trust the old launcher's won't trust it.");
        println!("Run again with --force to use the old launcher's, then restart the server.");
    }
    Ok(())
}

fn pin(args: &[String]) -> Result<(), String> {
    let config = config(args)?;
    let admin = Admin::open(&config.data_dir, std::sync::Arc::new(tvl_kit::time::SystemClock)).map_err(|e| e.to_string())?;
    let pin = if flag(args, "--reset") { admin.reset_pin().map_err(|e| e.to_string())? } else { admin.pin() };
    println!("Admin PIN: {pin}");
    Ok(())
}

#[cfg(windows)]
fn install(args: &[String]) -> Result<(), String> {
    let c = config(args)?;
    tvl_server::service::install(&c.data_dir)?;
    println!("Installed and started the {} service (data in {}).", tvl_server::service::NAME, c.data_dir.display());
    println!("The admin PIN: `tvl-server pin`. Phones and tablets: http://<this laptop>:{}", c.http_port);
    Ok(())
}
