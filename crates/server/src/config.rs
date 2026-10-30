//! Where the server keeps its data and which ports it takes. Three presets (the service, a console
//! run next to the old launcher, tests), then `server.toml` in the data folder for anything else.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use tvl_kit::Result;

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    /// The address listened on: every interface, or loopback in tests.
    pub bind: IpAddr,
    /// The companion and the API over HTTP. 0 picks a free port (tests).
    pub http_port: u16,
    /// The same over HTTPS, for pages that use the microphone. None: no HTTPS.
    pub https_port: Option<u16>,
    /// UDP port TV PCs ask "where is the server?" on. None: no answers.
    pub probe_port: Option<u16>,
    /// Host names the server is reached by besides its IP addresses, this PC's name and
    /// localhost (e.g. "tvl.local"). Requests naming any other host are refused (DNS rebinding),
    /// and HTTPS certificates name these too.
    pub names: Vec<String>,
    /// The built companion. None: `companion` next to the exe, else the repo's build.
    pub companion_dir: Option<PathBuf>,
    /// Next to the old launcher: no smart-home LAN, automations, ringing or calendar cards, so
    /// the two never fight over the devices or ring twice.
    pub shadow: bool,
    /// Lets the smart-home LAN run although the old launcher seems to be running.
    pub force_lan: bool,
    /// Pages opened on the server itself (http://localhost) count as admin.
    pub trust_loopback: bool,
    /// How long a TV PC that dropped its socket still shows as on, in case it's straight back.
    pub offline_grace: Duration,
}

/// The fields `server.toml` may set.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct File {
    http_port: Option<u16>,
    https_port: Option<u16>,
    probe_port: Option<u16>,
    names: Vec<String>,
    companion_dir: Option<PathBuf>,
}

impl Config {
    /// The service: /var/lib/tvl-server (C:\ProgramData\TVLauncher\server on Windows), ports
    /// 47960-47962.
    pub fn service() -> Self {
        #[cfg(windows)]
        let data_dir = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
            .join("TVLauncher")
            .join("server");
        #[cfg(not(windows))]
        let data_dir = PathBuf::from("/var/lib/tvl-server");
        Self::with(data_dir, 47960)
    }

    /// A console run on the dev PC next to the old launcher (which holds 47960/47961): data in
    /// %LOCALAPPDATA%\TVLauncher\server-dev, ports 47970-47972.
    pub fn dev() -> Self {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        Self::with(local.join("TVLauncher").join("server-dev"), 47970)
    }

    /// Loopback only, free ports, no HTTPS or probe.
    pub fn for_tests(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            http_port: 0,
            https_port: None,
            probe_port: None,
            names: Vec::new(),
            companion_dir: None,
            shadow: false,
            force_lan: false,
            trust_loopback: false,
            offline_grace: Duration::from_millis(50),
        }
    }

    fn with(data_dir: PathBuf, base: u16) -> Self {
        Self {
            data_dir,
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            http_port: base,
            https_port: Some(base + 1),
            probe_port: Some(base + 2),
            names: Vec::new(),
            companion_dir: None,
            shadow: false,
            force_lan: false,
            trust_loopback: true,
            offline_grace: Duration::from_secs(5),
        }
    }

    /// Applies `server.toml` from the data folder, when there is one.
    pub fn with_file(mut self) -> Result<Self> {
        let path = self.data_dir.join("server.toml");
        let Ok(text) = std::fs::read_to_string(&path) else { return Ok(self) };
        let f: File = toml::from_str(&text)?;
        if let Some(p) = f.http_port {
            self.http_port = p;
        }
        if f.https_port.is_some() {
            self.https_port = f.https_port;
        }
        if f.probe_port.is_some() {
            self.probe_port = f.probe_port;
        }
        self.names.extend(f.names.into_iter().map(|n| n.trim().to_ascii_lowercase()).filter(|n| !n.is_empty()));
        if f.companion_dir.is_some() {
            self.companion_dir = f.companion_dir;
        }
        Ok(self)
    }

    /// The built companion's folder.
    pub fn companion(&self) -> PathBuf {
        if let Some(d) = &self.companion_dir {
            return d.clone();
        }
        let beside_exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("companion")));
        if let Some(d) = beside_exe.filter(|d| d.join("index.html").is_file()) {
            return d;
        }
        // A debug build run from the repo serves the companion's latest build.
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/companion/dist")
    }

    pub fn logs(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_toml_adds_names_and_ports() {
        let dir = tvl_kit::testing::temp_dir("config");
        std::fs::write(dir.join("server.toml"), "http_port = 8080\nnames = [\"TVL.local\", \" \"]\n").unwrap();
        let c = Config::for_tests(&dir).with_file().unwrap();
        assert_eq!(c.http_port, 8080);
        assert_eq!(c.names, ["tvl.local"]);
        std::fs::write(dir.join("server.toml"), "htp_port = 1\n").unwrap();
        assert!(Config::for_tests(&dir).with_file().is_err(), "a misspelt setting is an error, not ignored");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_dev_preset_stays_off_the_old_launchers_ports() {
        let dev = Config::dev();
        assert_eq!((dev.http_port, dev.https_port, dev.probe_port), (47970, Some(47971), Some(47972)));
        let service = Config::service();
        assert_eq!((service.http_port, service.https_port), (47960, Some(47961)));
    }
}
