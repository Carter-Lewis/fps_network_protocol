use std::net::{SocketAddr};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256};
use sha2::{Sha256, Digest};

use bytes::Bytes;
use quinn::{Endpoint, ServerConfig, TransportConfig};
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

type UdpClients = Arc<Mutex<HashMap<SocketAddr, u16>>>;

fn make_or_load_cert() -> (Vec<u8>, Vec<u8>, String) {
    let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)
        .expect("ECDSA keygen failed");

    let mut params = CertificateParams::new(vec!["localhost".into()])
        .expect("cert params");

    // Chrome enforces <= 14 days for cert hash
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + time::Duration::days(13);

    let cert = params.self_signed(&key_pair).expect("self-signed");
    let cert_der = cert.der().to_vec();
    let key_der = key_pair.serialize_der();

    let mut hasher = Sha256::new();
    hasher.update(&cert_der);
    let hash = hasher.finalize();
    let b64 = base64::engine::general_purpose::STANDARD.encode(hash);

    println!("[CERT] SHA-256 fingerprint (base64): {}", b64);
    println!("[CERT] Paste this into webtransport_bridge.js as the certificate hash");
    (cert_der, key_der, b64)
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
    server_crypto.alpn_protocols = vec![b"hq-29".to_vec()];

    let mut server_config = ServerConfig::with_crypto(Arc::new(server_crypto));

    let mut transport_config = TransportConfig::default();
    transport_config.datagram_receive_buffer_size(Some(64 * 1024));
    *Arc::get_mut(&mut server_config.transport).expect("transport config still shared") = transport_config;

    server_config
}

fn snapshot_world(players: &Players) -> Vec<u8> {
    let players = players.lock().unwrap();
    WorldState {
        players: players.values().map(|p| PlayerState {
            player_id: p.id,
            pos_x: p.pos[0],
            pos_y: p.pos[1],
            pos_z: p.pos[2],
            yaw: p.yaw,
            pitch: p.pitch,
            health: p.health,
        }).collect(),
    }.serialize()
}

async fn run_legacy_tcp_udp(players: Players, udp_clients: UdpClients) {
    let tcp_listener = match TcpListener::bind("0.0.0.0:7777").await {
        Ok(listener) => listener,
        Err(e) => {
            println!("[!] Failed to bind legacy TCP listener: {}", e);
            return;
        }
    };

    let udp_recv = match UdpSocket::bind("0.0.0.0:7778").await {
        Ok(socket) => socket,
        Err(e) => {
            println!("[!] Failed to bind legacy UDP listener: {}", e);
            return;
        }
    };

    let udp_send = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(socket) => socket,
        Err(e) => {
            println!("[!] Failed to bind legacy UDP send socket: {}", e);
            return;
        }
    };

    println!("[*] Legacy TCP listening on 0.0.0.0:7777");
    println!("[*] Legacy UDP listening on 0.0.0.0:7778");

    let players_for_broadcast = players.clone();
    let udp_clients_for_broadcast = udp_clients.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            interval.tick().await;
            let bytes = snapshot_world(&players_for_broadcast);
            let clients: Vec<SocketAddr> = {
                let map = udp_clients_for_broadcast.lock().unwrap();
                map.keys().cloned().collect()
            };
            for addr in clients {
                let _ = udp_send.send_to(&bytes, addr).await;
            }
        }
    });

    let players_for_udp = players.clone();
    let udp_clients_for_udp = udp_clients.clone();
    tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        loop {
            match udp_recv.recv_from(&mut buf).await {
                Ok((n, src)) => {
                    if n == 0 {
                        continue;
                    }

                    let data = &buf[..n];
                    if data[0] == MSG_PLAYER_INPUT {
                        if let Some(input) = PlayerInput::deserialize(data) {
                            let pid = {
                                let map = udp_clients_for_udp.lock().unwrap();
                                map.get(&src).copied()
                            };

                            if let Some(pid) = pid {
                                let mut players = players_for_udp.lock().unwrap();
                                if let Some(player) = players.get_mut(&pid) {
                                    let speed = 0.1;
                                    let yaw = player.yaw;
                                    player.pos[0] += (input.move_x as f32 * yaw.cos() + input.move_z as f32 * yaw.sin()) * speed;
                                    player.pos[2] += (input.move_z as f32 * yaw.cos() - input.move_x as f32 * yaw.sin()) * speed;
                                    player.yaw = input.yaw;
                                    player.pitch = input.pitch;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("[!] Legacy UDP recv error: {}", e);
                }
            }
        }
    });

    loop {
        match tcp_listener.accept().await {
            Ok((mut stream, peer)) => {
                let players = players.clone();
                let udp_clients = udp_clients.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 3];
                    if let Err(e) = stream.read_exact(&mut buf).await {
                        println!("[!] Failed to read legacy connect packet: {}", e);
                        return;
                    }

                    if let Some(connect) = Connect::deserialize(&buf) {
                        let player_id = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed);
                        let udp_addr = SocketAddr::new(peer.ip(), connect.udp_port);

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

                        udp_clients.lock().unwrap().insert(udp_addr, player_id);

                        let response = Connected { player_id };
                        if let Err(e) = stream.write_all(&response.serialize()).await {
                            println!("[!] Failed to send legacy CONNECTED: {}", e);
                        } else {
                            println!("[+] Legacy client {} connected as player {}", peer, player_id);
                        }
                    }
                });
            }
            Err(e) => {
                println!("[!] Legacy TCP accept error: {}", e);
            }
        }
    }
}

async fn handle_quic_client(connection: quinn::Connection, players: Players) {
    println!("[+] Handling QUIC client: {}", connection.remote_address());

    // Map this QUIC connection to a player once we receive MSG_CONNECT.
    let mut player_id: Option<u16> = None;

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
                        if bytes.is_empty() {
                            continue;
                        }

                        match bytes[0] {
                            MSG_CONNECT => {
                                if player_id.is_some() {
                                    continue;
                                }

                                if let Some(_connect) = Connect::deserialize(&bytes) {
                                    let new_player_id = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed);
                                    println!("[+] Assigned PlayerID {}", new_player_id);

                                    {
                                        let mut players = players.lock().unwrap();
                                        players.insert(new_player_id, Player {
                                            id: new_player_id,
                                            pos: [0.0, 0.0, 0.0],
                                            yaw: 0.0,
                                            pitch: 0.0,
                                            health: 100,
                                            alive: true,
                                        });
                                    }

                                    player_id = Some(new_player_id);

                                    let response = Connected { player_id: new_player_id };
                                    if let Err(e) = connection.send_datagram(response.serialize().into()) {
                                        println!("[!] Failed to send CONNECTED datagram: {}", e);
                                    } else {
                                        println!("[+] Sent CONNECTED with Player ID {}", new_player_id);
                                    }
                                }
                            }
                            MSG_PLAYER_INPUT => {
                                if let Some(input) = PlayerInput::deserialize(&bytes) {
                                    if let Some(pid) = player_id {
                                        let mut players = players.lock().unwrap();
                                        if let Some(p) = players.get_mut(&pid) {
                                            let speed = 0.1;
                                            let yaw = p.yaw;
                                            p.pos[0] += (input.move_x as f32 * yaw.cos() + input.move_z as f32 * yaw.sin()) * speed;
                                            p.pos[2] += (input.move_z as f32 * yaw.cos() - input.move_x as f32 * yaw.sin()) * speed;
                                            p.yaw = input.yaw;
                                            p.pitch = input.pitch;
                                            println!("[>] Player {} moved to {:?}", p.id, p.pos);
                                        }
                                    }
                                }
                            }
                            _ => {
                                println!("[>] Unknown datagram message type: {}", bytes[0]);
                            }
                        }
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
    let udp_clients: UdpClients = Arc::new(Mutex::new(HashMap::new()));

    let addr: SocketAddr = "0.0.0.0:7777"
        .parse()
        .expect("invalid server address");

    let server_config = make_server_config();

    let endpoint = Endpoint::server(server_config, addr)
        .expect("failed to start QUIC server");

    println!("[*] QUIC server listening on {}", addr);

    tokio::spawn(run_legacy_tcp_udp(players.clone(), udp_clients.clone()));

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
