use quinn::{Connection, Endpoint, ServerConfig, TransportConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

/// WebTransport Gateway
/// Accepts WebTransport connections (HTTP/3 over QUIC)
/// and forwards datagrams to the backend game server on 127.0.0.1:7777

#[tokio::main]
async fn main() {
    let gateway_addr: SocketAddr = "0.0.0.0:7776".parse().unwrap();
    let backend_addr: SocketAddr = "127.0.0.1:7777".parse().unwrap();

    // Create QUIC server config with datagram support for WebTransport
    let server_config = make_server_config();

    let endpoint_config = quinn::EndpointConfig::default();
    
    // Create UDP socket for QUIC
    let socket = std::net::UdpSocket::bind(gateway_addr)
        .expect("Failed to bind UDP socket");
    socket.set_nonblocking(true)
        .expect("Failed to set non-blocking");

    // Create endpoint
    let endpoint = match Endpoint::new(
        endpoint_config,
        Some(server_config),
        socket,
        Arc::new(quinn::TokioRuntime),
    ) {
        Ok(ep) => ep,
        Err(e) => {
            eprintln!("[Gateway] Failed to create endpoint: {}", e);
            return;
        }
    };

    println!("[WebTransport Gateway] Listening on {}", gateway_addr);
    println!("[WebTransport Gateway] Forwarding to backend: {}", backend_addr);
    println!("[WebTransport Gateway] TLS cert: self-signed for localhost");
    println!("[WebTransport Gateway] QUIC/HTTP3 with datagram support enabled");

    // Accept incoming connections
    while let Some(incoming) = endpoint.accept().await {
        let backend_addr = backend_addr.clone();
        tokio::spawn(async move {
            match incoming.await {
                Ok(conn) => {
                    println!("[Gateway] New connection from: {}", conn.remote_address());
                    if let Err(e) = handle_connection(conn, backend_addr).await {
                        eprintln!("[Gateway] Connection handler error: {}", e);
                    }
                }
                Err(e) => eprintln!("[Gateway] Connection error: {}", e),
            }
        });
    }
}

fn make_server_config() -> ServerConfig {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .expect("failed to generate self signed cert");

    let cert_der = cert.serialize_der().expect("failed to serialize der");
    let key_der = cert.serialize_private_key_der();

    let cert_chain = vec![rustls::Certificate(cert_der)];
    let key = rustls::PrivateKey(key_der);

    let mut server_crypto = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .expect("failed to build rustls server config");
    
    // Set ALPN for HTTP/3 (WebTransport compatible)
    server_crypto.alpn_protocols = vec![b"hq-29".to_vec()];

    let mut server_config = ServerConfig::with_crypto(Arc::new(server_crypto));

    let mut transport_config = TransportConfig::default();
    transport_config.datagram_receive_buffer_size(Some(64 * 1024));
    *Arc::get_mut(&mut server_config.transport).expect("transport config still shared") = transport_config;

    server_config
}

async fn handle_connection(
    conn: Connection,
    backend_addr: std::net::SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create a UDP socket to forward to backend
    let backend_socket = UdpSocket::bind("0.0.0.0:0").await?;
    backend_socket.connect(backend_addr).await?;
    let backend_socket = Arc::new(backend_socket);

    let player_id: u32 = (conn.stable_id() as u32) % 1000;
    println!("[Gateway] Assigned player_id: {}", player_id);

    // Handle both datagrams and reliable streams
    let conn_clone = conn.clone();
    let backend_clone = backend_socket.clone();
    let datagram_handle = tokio::spawn(async move {
        handle_datagrams(conn_clone, backend_clone).await
    });

    let conn_clone = conn.clone();
    let backend_clone = backend_socket.clone();
    let stream_handle = tokio::spawn(async move {
        handle_streams(conn_clone, backend_clone).await
    });

    // Wait for either task to finish
    tokio::select! {
        result = datagram_handle => {
            eprintln!("[Gateway] Datagram handler finished: {:?}", result);
        }
        result = stream_handle => {
            eprintln!("[Gateway] Stream handler finished: {:?}", result);
        }
    }

    println!("[Gateway] Connection closed");
    Ok(())
}

async fn handle_datagrams(conn: Connection, backend: Arc<UdpSocket>) {
    loop {
        match conn.read_datagram().await {
            Ok(datagram) => {
                // Forward datagram to backend game server
                if let Err(e) = backend.send(&datagram).await {
                    eprintln!("[Gateway] Failed to forward datagram: {}", e);
                    break;
                }
            }
            Err(e) => {
                // Connection closed or error
                eprintln!("[Gateway] Datagram read error: {}", e);
                break;
            }
        }
    }
}

async fn handle_streams(conn: Connection, backend: Arc<UdpSocket>) {
    // Optionally handle reliable uni streams as fallback
    loop {
        match conn.accept_uni().await {
            Ok(mut stream) => {
                let backend = backend.clone();
                tokio::spawn(async move {
                    // Read the entire stream and forward as datagram
                    if let Ok(data) = stream.read_to_end(usize::MAX).await {
                        if let Err(e) = backend.send(&data).await {
                            eprintln!("[Gateway] Failed to forward stream data: {}", e);
                        }
                    }
                });
            }
            Err(_) => break,
        }
    }
}
