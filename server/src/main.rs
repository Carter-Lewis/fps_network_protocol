use std::net::{SocketAddr};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU16, Ordering, AtomicU32};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256};
use sha2::{Sha256, Digest};
use wtransport::{Endpoint, Identity, ServerConfig, tls::Certificate, tls::PrivateKey};

use protocol::*;
use base64::Engine;
use time::{OffsetDateTime, Duration as TimeDuration};
use tokio::net::TcpStream;

// Global PlayerID counter
static NEXT_PLAYER_ID: AtomicU16 = AtomicU16::new(1);

// Global tick counter
static WORLD_TICK: AtomicU32 = AtomicU32::new(0);

// Player struct
struct Player {
    id: u16,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    health: i32,
    alive: bool,
    udp_addr: Option<SocketAddr>,
    tcp_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    wt_conn: Option<wtransport::Connection>,
}

impl Player {
    async fn send_reliable(&self, data: Vec<u8>) {
        if let Some(c) = &self.wt_conn {
            // open a uni stream for reliable delivery
            // if let Ok(mut s) = c.open_uni().await.unwrap_or_else(|e| {
                // println!("[!] open_uni failed: {e}"); panic!()
            // }).await {
                // let _ = s.write_all(&data).await;
                // let _ = s.finish().await;
            // }
            if let Ok(opening) = c.open_uni().await {
                if let Ok(mut s) = opening.await {
                    let _ = s.write_all(&data).await;
                    let _ = s.finish().await;
                }
            }
        } else if let Some(tx) = &self.tcp_tx {
            let _ = tx.send(data);
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

// Function to help movement feel less laggy
fn apply_movement(p: &mut Player, input: &PlayerInput) {
    let speed = 6.0; // matches Godot speed
    let delta = 1.0 / 60.0; // assumes ~60fps
    let input_x = input.move_x as f32;
    let input_z = input.move_z as f32;

    // normalize input
    let len = (input_x * input_x + input_z * input_z).sqrt();
    let (dir_x, dir_z) = if len > 0.0 {
        (input_x / len, input_z / len)
    } else {
        (0.0, 0.0)
    };

    // rotate by yaw
    let yaw = input.yaw;
    let world_x = dir_x * yaw.cos() - dir_z * yaw.sin();
    let world_z = dir_x * yaw.sin() + dir_z * yaw.cos();

    // apply movement
    p.pos[0] += world_x * speed * delta;
    p.pos[2] += world_z * speed * delta;

    // sync rotation
    p.yaw = input.yaw;
    p.pitch = input.pitch;

    // vertical position
    p.pos[1] = input.pos_y;
}

fn make_or_load_cert() -> (Vec<u8>, Vec<u8>, String) {
    const CERT_FILE: &str = "cert.der";
    const KEY_FILE: &str = "key.der";
    const EXPIRY_FILE: &str = "cert_expiry.txt";

    // Reuse saved cert if it has more than 1 day remaining
    if let (Ok(cert_der), Ok(key_der), Ok(expiry_str)) = (
        std::fs::read(CERT_FILE),
        std::fs::read(KEY_FILE),
        std::fs::read_to_string(EXPIRY_FILE),
    ) {
        if let Ok(expiry_unix) = expiry_str.trim().parse::<i64>() {
            let now_unix = OffsetDateTime::now_utc().unix_timestamp();
            let days_left = (expiry_unix - now_unix) / 86400;
            if days_left > 1 {
                let mut hasher = Sha256::new();
                hasher.update(&cert_der);
                let b64 = base64::engine::general_purpose::STANDARD.encode(hasher.finalize());
                println!("[CERT] Reusing saved cert ({} days remaining)", days_left);
                println!("[CERT] SHA-256 fingerprint (base64): {}", b64);
                return (cert_der, key_der, b64);
            }
            println!("[CERT] Saved cert expires too soon, regenerating...");
        }
    }

    // Generate new cert (Chrome enforces <= 14 days for hash-pinned certs)
    let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("ECDSA keygen failed");
    let now = OffsetDateTime::now_utc();
    let mut params = CertificateParams::new(vec!["localhost".into()]).expect("cert params");
    params.not_before = now;
    params.not_after = now + TimeDuration::days(13);

    let cert = params.self_signed(&key_pair).unwrap();
    let cert_der = cert.der().to_vec();
    let key_der = key_pair.serialize_der();

    let _ = std::fs::write(CERT_FILE, &cert_der);
    let _ = std::fs::write(KEY_FILE, &key_der);
    let _ = std::fs::write(EXPIRY_FILE, (now + TimeDuration::days(13)).unix_timestamp().to_string());

    // Write PEM files so serve.py can use them for HTTPS static file serving
    let cert_b64 = base64::engine::general_purpose::STANDARD.encode(&cert_der);
    let cert_pem = format!("-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        cert_b64.as_bytes().chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>().join("\n"));
    let key_b64 = base64::engine::general_purpose::STANDARD.encode(&key_der);
    let key_pem = format!("-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        key_b64.as_bytes().chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>().join("\n"));
    let _ = std::fs::write("cert.pem", &cert_pem);
    let _ = std::fs::write("key.pem", &key_pem);

    let mut hasher = Sha256::new();
    hasher.update(&cert_der);
    let b64 = base64::engine::general_purpose::STANDARD.encode(hasher.finalize());

    println!("[CERT] Generated new cert (valid 13 days)");
    println!("[CERT] SHA-256 fingerprint (base64): {}", b64);
    println!("[CERT] Paste this into NetworkManager.gd as CERT_HASH_B64");
    (cert_der, key_der, b64)
}

async fn build_endpoint() -> Endpoint<wtransport::endpoint::endpoint_side::Server> {
    // If DOMAIN is set, load the Let's Encrypt cert issued by certbot.
    // Otherwise fall back to the self-signed cert (local dev / no domain).
    let identity = match std::env::var("DOMAIN") {
        Ok(domain) => {
            let cert_path = format!("/etc/letsencrypt/live/{domain}/fullchain.pem");
            let key_path  = format!("/etc/letsencrypt/live/{domain}/privkey.pem");
            println!("[CERT] Loading Let's Encrypt cert for {domain}");
            Identity::load_pemfiles(&cert_path, &key_path)
                .await
                .unwrap_or_else(|e| panic!("[CERT] Failed to load cert for {domain}: {e}"))
        }
        Err(_) => {
            let (cert_der, key_der, _hash_b64) = make_or_load_cert();
            Identity::new(
                wtransport::tls::CertificateChain::single(
                    Certificate::from_der(cert_der).unwrap(),
                ),
                PrivateKey::from_der_pkcs8(key_der),
            )
        }
    };

    let config = ServerConfig::builder()
        .with_bind_default(7777)
        .with_identity(identity)
        .keep_alive_interval(Some(Duration::from_secs(3)))
        .build();

    Endpoint::server(config).expect("endpoint")
}

async fn broadcast_loop(players: Players) {
    let mut tick = tokio::time::interval(Duration::from_millis(16)); // changed from 50 to 16 to help with lag
    loop {
        tick.tick().await;
        let snapshot = snapshot_world(&players);
        let players_g = players.lock().unwrap();
        for p in players_g.values() {
            if let Some(c) = &p.wt_conn {
                let _ = c.send_datagram(snapshot.clone());
            }
            // legacy UDP clients are handled by run_legacy_tcp_udp's own broadcast
        }
    }
}

fn snapshot_world(players: &Players) -> Vec<u8> {
    let players = players.lock().unwrap();
    WorldState {
        tick: WORLD_TICK.fetch_add(1, Ordering::Relaxed),
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
        let mut interval = tokio::time::interval(Duration::from_millis(16)); // changed from 50 to 16 to help with lag
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
                                    // let speed = 0.1;
                                    // let yaw = player.yaw;
                                    // player.pos[0] += (input.move_x as f32 * yaw.cos() + input.move_z as f32 * yaw.sin()) * speed;
                                    // player.pos[2] += (input.move_z as f32 * yaw.cos() - input.move_x as f32 * yaw.sin()) * speed;
                                    // player.yaw = input.yaw;
                                    // player.pitch = input.pitch;
                                    apply_movement(player, &input);
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

                    let Some(connect) = Connect::deserialize(&buf) else { return; };

                    let player_id = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed);
                    let udp_addr = SocketAddr::new(peer.ip(), connect.udp_port);

                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

                    {
                        let mut players_g = players.lock().unwrap();
                        players_g.insert(player_id, Player {
                            id: player_id,
                            pos: [0.0, 0.0, 0.0],
                            yaw: 0.0,
                            pitch: 0.0,
                            health: 100,
                            alive: true,
                            tcp_tx: Some(tx),
                            udp_addr: Some(udp_addr),
                            wt_conn: None,
                        });
                    }
                    udp_clients.lock().unwrap().insert(udp_addr, player_id);

                    if let Err(e) = stream.write_all(&Connected { player_id }.serialize()).await {
                        println!("[!] Failed to send legacy CONNECTED: {}", e);
                        players.lock().unwrap().remove(&player_id);
                        udp_clients.lock().unwrap().remove(&udp_addr);
                        return;
                    }
                    println!("[+] Legacy client {} connected as player {}", peer, player_id);

                    let (mut read_half, mut write_half) = stream.into_split();

                    // Write task: drain the channel to the socket
                    tokio::spawn(async move {
                        while let Some(data) = rx.recv().await {
                            if write_half.write_all(&data).await.is_err() {
                                break;
                            }
                        }
                    });

                    // Read loop: handle reliable messages until disconnect
                    let mut header = [0u8; 1];
                    loop {
                        if read_half.read_exact(&mut header).await.is_err() {
                            break;
                        }
                        match header[0] {
                            MSG_SWING => {
                                let mut rest = [0u8; 2];
                                if read_half.read_exact(&mut rest).await.is_err() { break; }
                                let mut msg = vec![header[0]];
                                msg.extend_from_slice(&rest);
                                handle_swing(&players, &msg).await;
                            }
                            MSG_RESPAWN_REQUEST => {
                                let mut rest = [0u8; 2];
                                if read_half.read_exact(&mut rest).await.is_err() { break; }
                                let mut msg = vec![header[0]];
                                msg.extend_from_slice(&rest);
                                handle_respawn(&players, &msg);
                            }
                            _ => break,
                        }
                    }

                    // Disconnect: remove player so world state stops broadcasting them
                    players.lock().unwrap().remove(&player_id);
                    udp_clients.lock().unwrap().remove(&udp_addr);
                    println!("[-] Legacy client {} (player {}) disconnected", peer, player_id);
                });
            }
            Err(e) => {
                println!("[!] Legacy TCP accept error: {}", e);
            }
        }
    }
}

/* commented out because not being used
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
                                                tcp_tx: None,
                                                udp_addr: None,
                                                wt_conn: None,
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
                                            tcp_tx: None,
                                            udp_addr: None,
                                            wt_conn: None,
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
                                            // let speed = 0.1;
                                            // let yaw = p.yaw;
                                            // p.pos[0] += (input.move_x as f32 * yaw.cos() + input.move_z as f32 * yaw.sin()) * speed;
                                            // p.pos[2] += (input.move_z as f32 * yaw.cos() - input.move_x as f32 * yaw.sin()) * speed;
                                            // p.yaw = input.yaw;
                                            // p.pitch = input.pitch;
                                            apply_movement(p, &input);
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
 */

// changed for lag helping
/*
fn apply_player_input(players: &Players, bytes: &[u8]) {
    if let Some(input) = PlayerInput::deserialize(bytes) {
        let mut players = players.lock().unwrap();
        if let Some(p) = players.get_mut(&input.player_id) {
            let yaw = p.yaw;
            p.pos[0] += (input.move_x as f32 * yaw.cos() + input.move_z as f32 * yaw.sin()) * 0.1;
            p.pos[2] += (input.move_z as f32 * yaw.cos() - input.move_x as f32 * yaw.sin()) * 0.1;
            p.pos[1] = input.pos_y;
            p.yaw = input.yaw;
            p.pitch = input.pitch;
        }
    }
}
 */
fn apply_player_input(players: &Players, bytes: &[u8]) {
    if let Some(input) = PlayerInput::deserialize(bytes) {
        let mut players = players.lock().unwrap();
        if let Some(p) = players.get_mut(&input.player_id) {
            apply_movement(p, &input);
        }
    }
}

async fn handle_swing(players: &Players, buf: &[u8]) {
    let Some(swing) = Swing::deserialize(buf) else { return; };

    let notify = SwingNotify { player_id: swing.player_id }.serialize();

    let victims: Vec<u16> = {
        let players_g = players.lock().unwrap();
        let Some(sp) = players_g.get(&swing.player_id).map(|p| p.pos) else { return; };
        players_g.values()
            .filter(|p| p.id != swing.player_id && p.alive)
            .filter(|p| {
                let dx = p.pos[0] - sp[0];
                let dy = p.pos[1] - sp[1];
                let dz = p.pos[2] - sp[2];
                (dx*dx + dy*dy + dz*dz).sqrt() < 2.0
            })
            .map(|p| p.id)
            .collect()
    };

    for victim_id in victims {
        let (new_health, died) = {
            let mut players_g = players.lock().unwrap();
            if let Some(p) = players_g.get_mut(&victim_id) {
                p.health -= 25;
                if p.health <= 0 { p.alive = false; }
                (p.health, p.health <= 0)
            } else {
                continue;
            }
        };

        let (victim_wt, victim_tcp) = {
            let players_g = players.lock().unwrap();
            players_g.get(&victim_id)
                .map(|p| (p.wt_conn.clone(), p.tcp_tx.clone()))
                .unwrap_or((None, None))
        };

        let health_msg = HealthUpdate { player_id: victim_id, health: new_health }.serialize();
        if let Some(vc) = &victim_wt {
            let _ = vc.send_datagram(health_msg.clone());
        } else if let Some(tx) = &victim_tcp {
            let _ = tx.send(health_msg);
        }

        if died {
            let died_msg = YouDied { player_id: victim_id }.serialize();
            if let Some(vc) = &victim_wt {
                if let Ok(opening) = vc.open_uni().await {
                    if let Ok(mut s) = opening.await {
                        let _ = s.write_all(&died_msg).await;
                        let _ = s.finish().await;
                    }
                }
            } else if let Some(tx) = &victim_tcp {
                let _ = tx.send(died_msg);
            }
        }
    }

    let players_g = players.lock().unwrap();
    for p in players_g.values() {
        if let Some(c) = &p.wt_conn {
            let _ = c.send_datagram(notify.clone());
        }
    }
}

fn handle_respawn(players: &Players, buf: &[u8]) {
    let Some(req) = RespawnRequest::deserialize(buf) else { return; };
    let mut players_g = players.lock().unwrap();
    if let Some(p) = players_g.get_mut(&req.player_id) {
        p.health = 100;
        p.alive = true;
        p.pos = [0.0, 0.0, 0.0];
    }
}

async fn broadcast_player_left(players: &Players, _udp: &Arc<UdpSocket>, _pid: u16) {
    let snapshot = snapshot_world(players);
    let players_g = players.lock().unwrap();
    for p in players_g.values() {
        if let Some(c) = &p.wt_conn {
            let _ = c.send_datagram(snapshot.clone());
        }
    }
}

async fn handle_wt_client(conn: wtransport::Connection, players: Players, udp: Arc<UdpSocket>) {
    let mut my_id: Option<u16> = None;

    loop {
        tokio::select! {
            // Datagrams: playerinput
            dgram = conn.receive_datagram() => {
                let Ok(d) = dgram else { break; };
                let bytes = d.payload();
                if bytes.is_empty() { continue; }
                match bytes[0] {
                    MSG_PLAYER_INPUT => apply_player_input(&players, &bytes),
                    _ => {} // Ignore case
                }
            }

            stream = conn.accept_uni() => {
                let Ok(mut s) = stream else {break; };
                let mut buf = Vec::new();
                s.read_to_end(&mut buf).await.unwrap_or_default();
                if buf.is_empty() { continue; }
                match buf[0] {
                    MSG_CONNECT => {
                        let pid = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed);
                        my_id = Some(pid);
                        players.lock().unwrap().insert(pid, Player {
                            id: pid, pos: [0.0;3], yaw: 0.0, pitch:0.0,
                            health: 100, alive: true,
                            udp_addr: None, tcp_tx: None,
                            wt_conn: Some(conn.clone()),
                        });
                        let resp = Connected {player_id: pid}.serialize();
                        println!("[+] WT player {pid} connected, sending Connected response...");
                        match conn.open_uni().await {
                            Err(e) => println!("[!] open_uni failed for player {pid}: {e}"),
                            Ok(opening) => match opening.await {
                                Err(e) => println!("[!] opening.await failed for player {pid}: {e}"),
                                Ok(mut out) => {
                                    if let Err(e) = out.write_all(&resp).await {
                                        println!("[!] write_all failed for player {pid}: {e}");
                                    } else if let Err(e) = out.finish().await {
                                        println!("[!] finish failed for player {pid}: {e}");
                                    } else {
                                        println!("[+] Connected response sent to player {pid}: {:?}", resp);
                                    }
                                }
                            }
                        }
                    }
                    MSG_SWING => handle_swing(&players, &buf).await,
                    MSG_RESPAWN_REQUEST => handle_respawn(&players, &buf),
                    _ => {}
                }
            }
        }
    }

    if let Some(pid) = my_id {
        players.lock().unwrap().remove(&pid);
        broadcast_player_left(&players, &udp, pid).await;
        println!("[-] WT player {pid} disconnected");
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
    tokio::spawn(broadcast_loop(players.clone()));

    let udp_wt = Arc::new(UdpSocket::bind("0.0.0.0:0").await.expect("bind wt udp"));

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
        let udp = udp_wt.clone();
        tokio::spawn(handle_wt_client(connection, players, udp));
    }
}
