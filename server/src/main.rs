use std::net::{SocketAddr};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256};
use sha2::{Sha256, Digest};
use wtransport::{Endpoint, Identity, ServerConfig, tls::Certificate, tls::PrivateKey};

use bytes::Bytes;
use protocol::*;
use base64::Engine;
use time::{OffsetDateTime, Duration as TimeDuration};

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
    udp_addr: Option<SocketAddr>,
    tcp_stream: Option<Arc<Mutex<TcpStream>>>,

    // wtransport path
    wt_conn: Option<wtransport::Connection>,
}

impl Player {
    async fn send_reliable(&self, data: Vec<u8>) {
        if let Some(c) = &self.wt_conn {
            // open a uni stream for reliable delivery
            if let Od(mut s) = c.open_uni().await.unwrap_or_else(|e| {
                println!("[!] open_uni failed: {e}"); panic!()
            }).await {
                let _ = s.write_all(&data).await;
                let _ = s.finish().await;
            }
        } else if let Some(tcp) = &self.tcp_stream {
            use std::io::Write;
            let _ = tcp_lock().unwrap().write_all(&data);
        }
    }

    fn send_unreliable(&self, data: Vec<u8>, udp: &Arc<UdpSocket>) {
        if let Some(c) = &self.wt_conn {
            let _ = c.send_datagram(data);
        } else if let Some(addr) = self.udp_addr {
            let _ = udp.try_send_to(&data, addr);
        }
    }
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
    let now = OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + TimeDuration::days(13);


    let cert = params.self_signed(&key_pair).unwrap();
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

async fn build_endpoint() -> Endpoint<wtransport::endpoint::endpoint_side::Server> {
    let (cert_der, key_der, _hash_b64) = make_or_load_cert();
    let identity = Identity::new(
        wtransport::tls::CertificateChain::single(Certificate::from_der(cert_der).unwrap()),
        PrivateKey::from_der_pkcs8(key_der),
    );

    let config = ServerConfig::builder()
        .with_bind_default(7777)
        .with_identity(identity)
        .keep_alive_interval(Some(Duration::from_secs(3)))
        .build();

    Endpoint:: server(config).expect("endpoint")
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

async fn _client(conn: wtransport::Connection, _players: Players) {
    println!("[+] WT client connected: {}", conn.remote_address());
    loop {
        tokio::select! {
            dgram = conn.receive_datagram() => match dgram {
                Ok(d) => println!("[>] datagram: {} bytes", d.payload().len()),
                Err(e) => {println!("[-] dgram closed: {e}"); break; }
            }, 
            stream = conn.accept_uni() => match stream {
                Ok(_s) => println!("[>] uni stream"),
                Err(e) => { println!("[-] stream accept closed: {e}"); break; }
            }
        }
    }
}


#[tokio::main]
async fn main() {
    let players: Players = Arc::new(Mutex::new(HashMap::new()));

    tokio::spawn(run_legacy_tcp_udp(players.clone(), Default::default()));

    let endpoint = build_endpoint().await;
    println!("[*] WebTransport server listening on UDP 7777");

    loop {
        let incoming = endpoint.accept().await;
        let session_request = match incoming.await {
            Ok(req) => req,
            Err(e) => {println!("[!] handshake failed: {e}"); continue; }
        };
        println!("[+] WT request: path={} authority={}",
            session_request.path(), session_request.authority());
        let connection = match session_request.accept().await {
            Ok(c) => c,
            Err(e) => {println!("[!] WT accept failed: {e}"); continue; }
        };
        let players = players.clone();
        tokio::spawn(_client(connection, players));
    }
}
