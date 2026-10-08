//! Invited QUIC sessions over validated local or received song packages

pub mod clock;
mod live;
mod resource;
mod session;
mod sync;
mod wire;

pub use live::{
    LiveCommand, LiveConfig, LiveEvent, LiveRole, LiveSendError, LiveSession, PhaseFrozen,
    PhaseObserved, PhasePublication, RecoveryFrozen, RecoveryObserved, RecoveryPublication,
};
pub use session::{SessionSummary, host, join, join_receive};
pub use sync::{ClockSample, NetworkTiming};

use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::Path,
    sync::Arc,
    time::Duration,
};

use quinn::{
    Connection, Endpoint, TransportConfig,
    crypto::rustls::{QuicClientConfig, QuicServerConfig},
    rustls::{self, pki_types::CertificateDer, pki_types::PrivatePkcs8KeyDer},
};
use serde::{Deserialize, Serialize};

pub(crate) const PROTOCOL_VERSION: u32 = 9;
const SERVER_NAME: &str = "cocobeat.local";
const ALPN: &[u8] = b"cocobeat-session/9";
const MAX_INVITE_BYTES: usize = 16 * 1024;
const MAX_CERTIFICATE_BYTES: usize = 4 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Invitation {
    pub invite_version: u32,
    pub protocol_version: u32,
    pub endpoint: String,
    pub server_name: String,
    pub certificate_der: Vec<u8>,
    pub cert_blake3: [u8; 32],
    pub token: [u8; 32],
    pub epoch: u64,
}

impl Invitation {
    fn validate(&self) -> Result<SocketAddr, String> {
        if self.invite_version != 1 || self.protocol_version != PROTOCOL_VERSION {
            return Err("unsupported invitation or protocol version".into());
        }
        if self.server_name != SERVER_NAME {
            return Err("invitation server name must be cocobeat.local".into());
        }
        if self.certificate_der.is_empty() || self.certificate_der.len() > MAX_CERTIFICATE_BYTES {
            return Err("invitation certificate must contain 1 to 4096 bytes".into());
        }
        if blake3::hash(&self.certificate_der).as_bytes() != &self.cert_blake3 {
            return Err("invitation certificate BLAKE3 mismatch".into());
        }
        let endpoint: SocketAddr = self
            .endpoint
            .parse()
            .map_err(|_| "invitation endpoint must be a SocketAddr")?;
        if !is_unicast(endpoint.ip()) || endpoint.port() == 0 {
            return Err(
                "invitation endpoint must specify a unicast address and nonzero port".into(),
            );
        }
        Ok(endpoint)
    }
}

fn is_unicast(ip: IpAddr) -> bool {
    let ip = ip.to_canonical();
    !ip.is_unspecified() && !ip.is_multicast() && ip != IpAddr::V4(Ipv4Addr::BROADCAST)
}

pub(crate) fn read_invite(path: &Path) -> Result<Invitation, String> {
    let metadata =
        std::fs::metadata(path).map_err(|error| format!("inspect invitation: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_INVITE_BYTES as u64 {
        return Err("invitation must be a regular file of at most 16 KiB".into());
    }
    let file = File::open(path).map_err(|error| format!("open invitation: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| format!("inspect opened invitation: {error}"))?
        .is_file()
    {
        return Err("invitation must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_INVITE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read invitation: {error}"))?;
    if bytes.len() > MAX_INVITE_BYTES {
        return Err("invitation exceeds 16 KiB".into());
    }
    // Keep attacker-controlled JSON values, including secrets, out of diagnostics
    let invitation: Invitation =
        serde_json::from_slice(&bytes).map_err(|_| "invalid invitation JSON")?;
    invitation.validate()?;
    Ok(invitation)
}

pub(crate) fn write_invite(path: &Path, invitation: &Invitation) -> Result<(), String> {
    invitation.validate()?;
    let bytes = serde_json::to_vec(invitation).map_err(|_| "serialize invitation failed")?;
    if bytes.len() > MAX_INVITE_BYTES {
        return Err("invitation exceeds 16 KiB".into());
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("create invitation: {error}"))?;
    let result = file.write_all(&bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = result {
        return match std::fs::remove_file(path) {
            Ok(()) => Err(format!("save invitation: {error}")),
            Err(cleanup) => Err(format!(
                "save invitation: {error}; remove partial: {cleanup}"
            )),
        };
    }
    Ok(())
}

fn transport(host: bool) -> Arc<TransportConfig> {
    let mut transport = TransportConfig::default();
    transport
        .max_concurrent_bidi_streams(if host { 2_u8.into() } else { 0_u8.into() })
        .max_concurrent_uni_streams(if host { 0_u8.into() } else { 2_u8.into() })
        .stream_receive_window((256_u32 * 1024).into())
        .receive_window((512_u32 * 1024).into())
        .send_window(1024 * 1024)
        .max_idle_timeout(Some(quinn::VarInt::from_u32(30_000).into()))
        .datagram_receive_buffer_size(Some(4 * 1024))
        .datagram_send_buffer_size(4 * 1024);
    Arc::new(transport)
}

pub(crate) fn continuation_capability() -> Result<[u8; 32], String> {
    let mut capability = [0; 32];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut capability)
        .map_err(|_| "continuation secure random generation failed")?;
    Ok(capability)
}

pub(crate) fn listen(bind: SocketAddr) -> Result<(Endpoint, Invitation), String> {
    if !is_unicast(bind.ip()) {
        return Err("host bind must specify a local unicast address".into());
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut token = [0; 32];
    let mut epoch = [0; 8];
    provider
        .secure_random
        .fill(&mut token)
        .and_then(|()| provider.secure_random.fill(&mut epoch))
        .map_err(|_| "session secure random generation failed")?;
    let certificate = rcgen::generate_simple_self_signed(vec![SERVER_NAME.into()])
        .map_err(|error| format!("generate session certificate: {error}"))?;
    let certificate_der = certificate.cert.der().to_vec();
    let key = PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der());
    let mut crypto = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| format!("configure server TLS: {error}"))?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate_der.clone())],
            key.into(),
        )
        .map_err(|error| format!("configure server certificate: {error}"))?;
    crypto.alpn_protocols = vec![ALPN.to_vec()];
    crypto.max_early_data_size = 0;
    let crypto = QuicServerConfig::try_from(crypto)
        .map_err(|error| format!("configure QUIC server TLS: {error}"))?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    config
        .transport_config(transport(true))
        .max_incoming(2)
        .incoming_buffer_size(32 * 1024)
        .incoming_buffer_size_total(64 * 1024);
    let endpoint =
        Endpoint::server(config, bind).map_err(|error| format!("bind host endpoint: {error}"))?;
    let invitation = Invitation {
        invite_version: 1,
        protocol_version: PROTOCOL_VERSION,
        endpoint: endpoint
            .local_addr()
            .map_err(|error| format!("read host endpoint address: {error}"))?
            .to_string(),
        server_name: SERVER_NAME.into(),
        cert_blake3: *blake3::hash(&certificate_der).as_bytes(),
        certificate_der,
        token,
        epoch: u64::from_le_bytes(epoch),
    };
    invitation.validate()?;
    Ok((endpoint, invitation))
}

pub(crate) async fn connect(invitation: &Invitation) -> Result<(Endpoint, Connection), String> {
    connect_owned(invitation, &mut None).await
}

pub(crate) async fn connect_owned(
    invitation: &Invitation,
    owned_endpoint: &mut Option<Endpoint>,
) -> Result<(Endpoint, Connection), String> {
    let peer = invitation.validate()?;
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from(invitation.certificate_der.clone()))
        .map_err(|error| format!("read invitation trust anchor: {error}"))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut crypto = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| format!("configure client TLS: {error}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    crypto.alpn_protocols = vec![ALPN.to_vec()];
    crypto.enable_early_data = false;
    let crypto = QuicClientConfig::try_from(crypto)
        .map_err(|error| format!("configure QUIC client TLS: {error}"))?;
    let mut config = quinn::ClientConfig::new(Arc::new(crypto));
    config.transport_config(transport(false));
    let bind = if peer.is_ipv4() {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
    };
    let mut endpoint = Endpoint::client(SocketAddr::new(bind, 0))
        .map_err(|error| format!("bind guest endpoint: {error}"))?;
    endpoint.set_default_client_config(config);
    *owned_endpoint = Some(endpoint.clone());
    let connecting = endpoint
        .connect(peer, SERVER_NAME)
        .map_err(|_| "connect to host failed")?;
    let connection = tokio::time::timeout(Duration::from_secs(10), connecting)
        .await
        .map_err(|_| "host TLS handshake timed out")?
        .map_err(|_| "host TLS handshake failed")?;
    let matches = connection
        .peer_identity()
        .and_then(|identity| identity.downcast::<Vec<CertificateDer<'static>>>().ok())
        .and_then(|certificates| certificates.first().cloned())
        .is_some_and(|leaf| leaf.as_ref() == invitation.certificate_der.as_slice());
    if !matches {
        connection.close(1_u8.into(), b"host certificate mismatch");
        return Err("host leaf certificate does not match invitation DER".into());
    }
    Ok((endpoint, connection))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_is_strict_bounded_and_never_overwrites() {
        let directory = std::env::temp_dir().join(format!(
            "cocobeat-invite-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        assert!(read_invite(&directory).is_err());
        let path = directory.join("invite.json");
        let invitation = Invitation {
            invite_version: 1,
            protocol_version: PROTOCOL_VERSION,
            endpoint: "127.0.0.1:12345".into(),
            server_name: SERVER_NAME.into(),
            certificate_der: vec![1, 2, 3],
            cert_blake3: *blake3::hash(&[1, 2, 3]).as_bytes(),
            token: [7; 32],
            epoch: 123,
        };
        write_invite(&path, &invitation).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert_eq!(read_invite(&path).unwrap().token, invitation.token);
        assert!(write_invite(&path, &invitation).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let value = serde_json::to_value(&invitation).unwrap();
        for endpoint in ["[::1]:12345", "[::ffff:127.0.0.1]:12345"] {
            let mut valid = value.clone();
            valid["endpoint"] = serde_json::json!(endpoint);
            std::fs::write(&path, serde_json::to_vec(&valid).unwrap()).unwrap();
            assert_eq!(read_invite(&path).unwrap().endpoint, endpoint);
        }
        for (field, invalid) in [
            ("invite_version", serde_json::json!(2)),
            ("protocol_version", serde_json::json!(1)),
            ("protocol_version", serde_json::json!(2)),
            ("protocol_version", serde_json::json!(3)),
            ("protocol_version", serde_json::json!(4)),
            ("endpoint", serde_json::json!("0.0.0.0:12345")),
            ("endpoint", serde_json::json!("[::]:12345")),
            ("endpoint", serde_json::json!("[::ffff:0.0.0.0]:12345")),
            ("endpoint", serde_json::json!("224.0.0.1:12345")),
            ("endpoint", serde_json::json!("[ff02::1]:12345")),
            ("endpoint", serde_json::json!("255.255.255.255:12345")),
            ("endpoint", serde_json::json!("[::ffff:224.0.0.1]:12345")),
            (
                "endpoint",
                serde_json::json!("[::ffff:255.255.255.255]:12345"),
            ),
            ("endpoint", serde_json::json!("127.0.0.1:0")),
            ("endpoint", serde_json::json!("localhost:12345")),
            ("server_name", serde_json::json!("other.local")),
            ("certificate_der", serde_json::json!([])),
            ("certificate_der", serde_json::json!([1, 2, 4])),
            ("certificate_der", serde_json::json!(vec![0; 4097])),
            ("token", serde_json::json!([1, 2, 3])),
            ("epoch", serde_json::json!(-1)),
            ("unknown", serde_json::json!(true)),
        ] {
            let mut invalid_value = value.clone();
            invalid_value[field] = invalid;
            std::fs::write(&path, serde_json::to_vec(&invalid_value).unwrap()).unwrap();
            assert!(read_invite(&path).is_err(), "accepted invalid {field}");
        }
        let duplicate = format!(
            "{{\"epoch\":1,{}",
            std::str::from_utf8(&original[1..]).unwrap()
        );
        std::fs::write(&path, duplicate).unwrap();
        assert!(read_invite(&path).is_err());
        std::fs::write(&path, vec![b' '; MAX_INVITE_BYTES + 1]).unwrap();
        assert!(read_invite(&path).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
