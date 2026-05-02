use std::{error::Error, net::SocketAddr, sync::Arc, time::Duration};

use protocol::{Connect, Connected, MSG_CONNECTED};
use quinn::{ClientConfig, Endpoint, TransportConfig};
use rustls::{client::ServerCertVerifier, Certificate, ClientConfig as RustlsClientConfig, Error as RustlsError, ServerName};

struct SkipServerVerification;

impl ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, RustlsError> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

fn make_client_config() -> ClientConfig {
    let mut tls_config = RustlsClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
        .with_no_client_auth();

    tls_config.alpn_protocols = vec![b"hq-29".to_vec()];

    let mut transport_config = TransportConfig::default();
    transport_config.datagram_receive_buffer_size(Some(64 * 1024));

    let mut client_config = ClientConfig::new(Arc::new(tls_config));
    client_config.transport_config(Arc::new(transport_config));
    client_config
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let server_addr: SocketAddr = "127.0.0.1:7777".parse()?;
    let local_addr: SocketAddr = "0.0.0.0:0".parse()?;

    let mut endpoint = Endpoint::client(local_addr)?;
    endpoint.set_default_client_config(make_client_config());

    println!("[quic_smoke] Connecting to {}", server_addr);
    let connecting = endpoint.connect(server_addr, "localhost")?;
    let connection = connecting.await?;
    println!("[quic_smoke] Connected from {}", connection.remote_address());

    let connect = Connect { udp_port: 0 };
    connection.send_datagram(connect.serialize().into())?;
    println!("[quic_smoke] Sent MSG_CONNECT datagram");

    let response = tokio::time::timeout(Duration::from_secs(5), connection.read_datagram()).await??;
    println!("[quic_smoke] Received {} bytes", response.len());

    if response.first().copied() != Some(MSG_CONNECTED) {
        return Err(format!("unexpected response type: {:?}", response.first()).into());
    }

    let connected = Connected::deserialize(&response).ok_or("failed to decode CONNECTED")?;
    println!("[quic_smoke] Server assigned player_id={}", connected.player_id);

    Ok(())
}