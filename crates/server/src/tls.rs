//! HTTPS, from the old launcher's `companion/tls.rs`. Browsers only let a page use the microphone
//! over HTTPS, so there a phone or tablet can talk to the assistant itself; TV PCs use it too, for
//! their socket to the server. The certificates come from a home certificate authority
//! (companion-ca.pem/.key in the data folder, copied over by the import so devices that already
//! trust it need nothing new), which each device trusts once: /ca.crt on the plain server hands
//! it out. Every address the server is reached at gets a certificate of its own the first time
//! it's asked for.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::serve::Listener;
use parking_lot::Mutex;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

use crate::http::security::is_private;

const CA_CERT: &str = "companion-ca.pem";
const CA_KEY: &str = "companion-ca.key";
/// A device that opens the connection and never finishes the handshake is let go after this.
const HANDSHAKE: Duration = Duration::from_secs(10);
/// Certificates are good for 397 days (Apple's limit is 398); made anew well before that.
const RENEW: Duration = Duration::from_secs(30 * 24 * 3600);

type Result<T> = std::result::Result<T, String>;

pub struct Authority {
    issuer: Issuer<'static, KeyPair>,
    /// The authority's own certificate, as devices install it.
    der: CertificateDer<'static>,
    pem: String,
    /// Host names the server answers to besides its addresses (server.toml `names`).
    names: Vec<String>,
    /// The server setup for each address, and when its certificate was made.
    configs: Mutex<HashMap<IpAddr, (Instant, Arc<ServerConfig>)>>,
}

/// What the authority's certificate says, the same each run: a certificate made today must
/// name as its issuer exactly the one the devices installed. Byte for byte the old launcher's,
/// so its authority, copied here, still matches.
fn ca_params() -> CertificateParams {
    let mut p = CertificateParams::default();
    p.distinguished_name = DistinguishedName::new();
    p.distinguished_name.push(DnType::CommonName, "TV Launcher home");
    p.distinguished_name.push(DnType::OrganizationName, "TV Launcher");
    // It signs the server's certificates and nothing below them.
    p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    p
}

impl Authority {
    /// The authority kept in `dir`, made there the first time.
    pub fn load(dir: &Path, names: &[String]) -> Result<Self> {
        let (cert_path, key_path) = (dir.join(CA_CERT), dir.join(CA_KEY));
        let (pem, key) = match (std::fs::read_to_string(&cert_path), std::fs::read_to_string(&key_path)) {
            (Ok(pem), Ok(key)) => (pem, KeyPair::from_pem(&key).map_err(|e| format!("{CA_KEY}: {e}"))?),
            _ => {
                let key = KeyPair::generate().map_err(|e| e.to_string())?;
                let mut p = ca_params();
                let now = OffsetDateTime::now_utc();
                p.not_before = now - time::Duration::days(1);
                p.not_after = now + time::Duration::days(20 * 365);
                let pem = p.self_signed(&key).map_err(|e| e.to_string())?.pem();
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                std::fs::write(&key_path, key.serialize_pem()).map_err(|e| format!("{CA_KEY}: {e}"))?;
                std::fs::write(&cert_path, &pem).map_err(|e| format!("{CA_CERT}: {e}"))?;
                tracing::info!("made a home certificate authority ({})", cert_path.display());
                (pem, key)
            }
        };
        let der = CertificateDer::from_pem_slice(pem.as_bytes()).map_err(|e| format!("{CA_CERT}: {e}"))?;
        Ok(Self { issuer: Issuer::new(ca_params(), key), der, pem, names: names.to_vec(), configs: Mutex::default() })
    }

    /// The authority's certificate (DER), for a device to install.
    pub fn certificate(&self) -> Vec<u8> {
        self.der.to_vec()
    }

    /// The same as PEM text, handed to a TV PC when it's paired.
    pub fn pem(&self) -> &str {
        &self.pem
    }

    /// SHA-256 of the certificate, "3F:9A:…", for comparing what a TV shows with what the server
    /// shows while pairing.
    pub fn fingerprint(&self) -> String {
        Sha256::digest(&self.der).iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
    }

    /// The server setup for connections to `ip`, with a certificate naming it.
    fn config(&self, ip: IpAddr) -> Result<Arc<ServerConfig>> {
        if let Some((made, c)) = self.configs.lock().get(&ip) {
            if made.elapsed() < RENEW {
                return Ok(c.clone());
            }
        }
        let (cert, key) = self.certificate_for(ip)?;
        let mut config = ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .and_then(|b| b.with_no_client_auth().with_single_cert(vec![cert], key))
            .map_err(|e| e.to_string())?;
        // HTTP/1.1 only: what the WebSocket upgrades from.
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let config = Arc::new(config);
        self.configs.lock().insert(ip, (Instant::now(), config.clone()));
        Ok(config)
    }

    /// A certificate for `ip`, this PC's name and the configured names, signed by the authority,
    /// and its key.
    fn certificate_for(&self, ip: IpAddr) -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>)> {
        let mut names = vec![ip.to_string(), "localhost".to_string()];
        if let Some(host) = hostname() {
            names.push(format!("{host}.local"));
            names.push(host);
        }
        for n in &self.names {
            if !names.contains(n) {
                names.push(n.clone());
            }
        }
        let mut p = CertificateParams::new(names).map_err(|e| e.to_string())?;
        p.distinguished_name = DistinguishedName::new();
        p.distinguished_name.push(DnType::CommonName, "TV Launcher companion");
        p.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        p.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        p.use_authority_key_identifier_extension = true;
        let now = OffsetDateTime::now_utc();
        p.not_before = now - time::Duration::days(1);
        p.not_after = now + time::Duration::days(397);
        let key = KeyPair::generate().map_err(|e| e.to_string())?;
        let cert = p.signed_by(&key, &self.issuer).map_err(|e| e.to_string())?;
        Ok((cert.der().clone(), PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()))))
    }
}

/// This PC's name as the home network knows it (DESKTOP-AB12 → desktop-ab12).
pub fn hostname() -> Option<String> {
    // systemd gives its services neither variable: Linux keeps the name in /proc.
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").ok())?
        .trim()
        .to_ascii_lowercase();
    let ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then_some(name)
}

/// Connections that finished the TLS handshake, to serve as any others.
pub struct Tls {
    ready: mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    local: SocketAddr,
}

/// Takes connections on `addr`, each handshake on its own task so a slow device holds up no one.
/// Only home addresses get as far as the handshake.
pub async fn listen(addr: SocketAddr, ca: Arc<Authority>) -> io::Result<Tls> {
    let tcp = TcpListener::bind(addr).await?;
    let local = tcp.local_addr()?;
    let (tx, ready) = mpsc::channel(32);
    tokio::spawn(async move {
        loop {
            let (stream, peer) = match tcp.accept().await {
                Ok(c) => c,
                // Out of sockets and the like: try again shortly.
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            if !is_private(peer.ip()) {
                continue;
            }
            let (ca, tx) = (ca.clone(), tx.clone());
            tokio::spawn(async move {
                let Ok(here) = stream.local_addr() else { return };
                let config = match ca.config(here.ip()) {
                    Ok(c) => c,
                    Err(e) => return tracing::warn!("no certificate for {}: {e}", here.ip()),
                };
                // A device that doesn't trust the authority yet hangs up here.
                if let Ok(Ok(tls)) = tokio::time::timeout(HANDSHAKE, TlsAcceptor::from(config).accept(stream)).await {
                    let _ = tx.send((tls, peer)).await;
                }
            });
        }
    });
    Ok(Tls { ready, local })
}

impl Tls {
    pub fn addr(&self) -> SocketAddr {
        self.local
    }
}

impl Listener for Tls {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(c) => c,
            // The accepting task never ends while the server runs.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local)
    }
}

#[cfg(test)]
mod tests {
    use rustls::client::danger::ServerCertVerifier;
    use rustls::client::WebPkiServerVerifier;
    use rustls::pki_types::{ServerName, UnixTime};

    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        tvl_kit::testing::temp_dir(name)
    }

    fn verifier(ca: &Authority) -> Arc<WebPkiServerVerifier> {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(CertificateDer::from(ca.certificate())).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider).build().unwrap()
    }

    #[test]
    fn certificates_check_out_against_the_installed_authority() {
        let dir = temp_dir("tls");
        let installed = Authority::load(&dir, &[]).unwrap();
        // The next run loads the same authority, and what it signs then still checks out.
        let next_run = Authority::load(&dir, &["tvl.local".into()]).unwrap();
        assert_eq!(installed.certificate(), next_run.certificate());
        assert_eq!(installed.fingerprint(), next_run.fingerprint());

        let verifier = verifier(&installed);
        let ip: IpAddr = "192.168.68.106".parse().unwrap();
        let (cert, _) = next_run.certificate_for(ip).unwrap();
        let name = ServerName::IpAddress(ip.into());
        verifier.verify_server_cert(&cert, &[], &name, &[], UnixTime::now()).unwrap();
        // The configured name is in it too.
        let by_name = ServerName::try_from("tvl.local").unwrap();
        verifier.verify_server_cert(&cert, &[], &by_name, &[], UnixTime::now()).unwrap();
        // Not for another address.
        let other = ServerName::IpAddress("192.168.68.107".parse::<IpAddr>().unwrap().into());
        assert!(verifier.verify_server_cert(&cert, &[], &other, &[], UnixTime::now()).is_err());
        assert!(next_run.config(ip).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_old_launchers_authority_is_used_as_it_is() {
        // An authority made by the old launcher's code: same parameters, its own key.
        let dir = temp_dir("tls-old");
        let key = KeyPair::generate().unwrap();
        let mut p = ca_params();
        p.not_before = OffsetDateTime::now_utc() - time::Duration::days(30);
        p.not_after = OffsetDateTime::now_utc() + time::Duration::days(3650);
        let pem = p.self_signed(&key).unwrap().pem();
        std::fs::write(dir.join(CA_KEY), key.serialize_pem()).unwrap();
        std::fs::write(dir.join(CA_CERT), &pem).unwrap();
        let ca = Authority::load(&dir, &[]).unwrap();
        assert_eq!(ca.pem(), pem, "loaded, not made anew");
        let ip: IpAddr = "10.0.0.5".parse().unwrap();
        let (cert, _) = ca.certificate_for(ip).unwrap();
        verifier(&ca).verify_server_cert(&cert, &[], &ServerName::IpAddress(ip.into()), &[], UnixTime::now()).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn serves_https_and_knows_who_asked() {
        use axum::extract::ConnectInfo;
        use axum::serve::ListenerExt;

        let dir = temp_dir("https");
        let ca = Arc::new(Authority::load(&dir, &[]).unwrap());
        let listener = listen("127.0.0.1:0".parse().unwrap(), ca.clone()).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(|ConnectInfo(peer): ConnectInfo<SocketAddr>| async move { peer.ip().to_string() }),
        );
        let app = app.into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(listener.tap_io(|_| {}), app).await });

        let url = format!("https://127.0.0.1:{port}/");
        let trusting = reqwest::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_der(&ca.certificate()).unwrap())
            .build()
            .unwrap();
        let said = trusting.get(&url).send().await.unwrap().text().await.unwrap();
        assert_eq!(said, "127.0.0.1");
        // A device that hasn't installed the authority gets no further than the handshake.
        assert!(reqwest::Client::new().get(&url).send().await.is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
