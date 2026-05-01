use std::net::{SocketAddr};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

use bytes::Bytes;
use quinn::{Endpoint, ServerConfig};
use protocol::*;

// Global PlayerID counter
static NEXT_PLAYER_ID: AtomicU16 = AtomicU16::new(1);

// Player struct
struct Player {
    id: u16,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    health: i32,
    alive: bool,
    //udp_addr: Option<SocketAddr>,
    //tcp_stream: Option<Arc<Mutex<TcpStream>>>,
}

// Shared game state
type Players = Arc<Mutex<HashMap<u16, Player>>>;


fn make_server_config() -> ServerConfig {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .expect("failed to generate self signed cert");

    let cert_der = cert.serialize_der().expect("failed to serialize der");

    let key_der = cert.serialize_private_key_der();

    let cert_chain = vec![rustls::Certificate(cert_der)];
    let key = rustls::PrivateKey(key_der);

    ServerConfig::with_single_cert(cert_chain, key).expect("failed to create QUIC server config")
}

async fn handle_quic_client(connection: quinn::Connection, players: Players) {
    println!("[+] Handling QUIC client: {}", connection.remote_address());

    loop {
        tokio::select! {
            stream_result = connection.accept_uni() => {
                match stream_result {
                    Ok(mut recv_stream) => {

                        match recv_stream.read_to_end(1024).await {
                            // look at the first byte of the message, if its connect, create a player and send back "connected"
                            Ok(buf) => {
                                if buf.is_empty() {
                                    continue;
                                }

                                let msg_type = buf[0];
                                println!("[>] Reliable message type received: {}", msg_type);

                                if msg_type == MSG_CONNECT {
                                    if let Some(connect) = Connect::deserialize(&buf) {
                                        let player_id = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed);

                                        println!("[+] Assigned PLayerID {}", player_id);

                                        {
                                            let mut players = players.lock().unwrap();

                                            players.insert(player_id, Player {
                                                id: player_id,
                                                pos: [0.0, 0.0, 0.0],
                                                yaw: 0.0,
                                                pitch: 0.0,
                                                health: 100,
                                                alive: true,
                                            });
                                        }

                                        let response = Connected { player_id };

                                        match connection.open_uni().await {
                                            Ok (mut send_stream) => {
                                                if let Err(e) = send_stream.write_all(&response.serialize()).await {
                                                    println!("[!] Failed to send CONNECTED response: {}", e);
                                                }

                                                if let Err(e) = send_stream.finish().await {
                                                    println!("[!] Failed to finish stream: {}", e);
                                                }

                                                println!("[+] Sent CONNECTED with Player ID {}", player_id);
                                            }
                                            Err(e) => {
                                                println!("[!] Failed to open response stream: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                println!("[!] Failed to read stream: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        println!("[-] QUIC stream closed: {}", e);
                        break;
                    }
                }
            }

            datagram_result = connection.read_datagram() => {
                match datagram_result {
                    Ok(bytes) => {
                        println!("[>] Received datagram: {:?}", bytes);
                    }
                    Err(e) => {
                        println!("[-] QUIC datagram closed: {}", e);
                        break;
                    }
                }
            }
        }
    }
}


#[tokio::main]
async fn main() {
    let players: Players = Arc::new(Mutex::new(HashMap::new()));

    let addr: SocketAddr = "0.0.0.0:7777"
        .parse()
        .expect("invalid server address");

    let server_config = make_server_config();

    let endpoint = Endpoint::server(server_config, addr)
        .expect("failed to start QUIC server");

    println!("[*] QUIC server listening on {}", addr);

    while let Some(connecting) = endpoint.accept().await {
        let players_clone = players.clone();

        tokio::spawn(async move {
            match connecting.await {
                Ok(connection) => {
                    handle_quic_client(connection, players_clone).await;

                    // next step: handle this player's streams/datagrams here
                }
                Err(e) => {
                    println!("[!] QUIC connection failed: {}", e);
                }
            }
        });
    }
}
