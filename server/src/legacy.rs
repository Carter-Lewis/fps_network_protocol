use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use protocol::*;
use crate::player::{Player, Players, UdpClients, apply_movement};
use crate::state::NEXT_PLAYER_ID;
use crate::game::{snapshot_world, handle_swing, handle_respawn};

/// Runs the legacy TCP (port 7777) + UDP (port 7778) server.
/// TCP carries reliable messages (connect, swing, respawn).
/// UDP carries unreliable player input and world state broadcasts.
pub async fn run_legacy_tcp_udp(players: Players, udp_clients: UdpClients) {
    let tcp_listener = match TcpListener::bind("0.0.0.0:7777").await {
        Ok(l) => l,
        Err(e) => { println!("[!] Failed to bind legacy TCP listener: {}", e); return; }
    };

    let udp_recv = match UdpSocket::bind("0.0.0.0:7778").await {
        Ok(s) => s,
        Err(e) => { println!("[!] Failed to bind legacy UDP listener: {}", e); return; }
    };

    let udp_send = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => { println!("[!] Failed to bind legacy UDP send socket: {}", e); return; }
    };

    println!("[*] Legacy TCP listening on 0.0.0.0:7777");
    println!("[*] Legacy UDP listening on 0.0.0.0:7778");

    // Broadcast world state to all legacy UDP clients at ~60Hz
    let players_bc = players.clone();
    let clients_bc = udp_clients.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(16));
        loop {
            interval.tick().await;
            let bytes = snapshot_world(&players_bc);
            let addrs: Vec<SocketAddr> = {
                let map = clients_bc.lock().expect("udp_clients lock poisoned");
                map.keys().cloned().collect()
            };
            for addr in addrs {
                let _ = udp_send.send_to(&bytes, addr).await;
            }
        }
    });

    // Receive and apply UDP PlayerInput datagrams
    let players_udp = players.clone();
    let clients_udp = udp_clients.clone();
    tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        loop {
            match udp_recv.recv_from(&mut buf).await {
                Ok((n, src)) => {
                    if n == 0 { continue; }
                    let data = &buf[..n];
                    if data[0] == MSG_PLAYER_INPUT {
                        if let Some(input) = PlayerInput::deserialize(data) {
                            let pid = {
                                let map = clients_udp.lock().expect("udp_clients lock poisoned");
                                map.get(&src).copied()
                            };
                            if let Some(pid) = pid {
                                let mut players = players_udp.lock().expect("players lock poisoned");
                                if let Some(player) = players.get_mut(&pid) {
                                    apply_movement(player, &input);
                                }
                            }
                        }
                    }
                }
                Err(e) => println!("[!] Legacy UDP recv error: {}", e),
            }
        }
    });

    // Accept incoming TCP connections
    loop {
        match tcp_listener.accept().await {
            Ok((mut stream, peer)) => {
                let players = players.clone();
                let udp_clients = udp_clients.clone();
                tokio::spawn(async move {
                    // Read the initial Connect packet (3 bytes)
                    let mut buf = [0u8; 3];
                    if let Err(e) = stream.read_exact(&mut buf).await {
                        println!("[!] Failed to read legacy connect packet: {}", e);
                        return;
                    }
                    let Some(connect) = Connect::deserialize(&buf) else { return; };

                    let player_id = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed);
                    let udp_addr = SocketAddr::new(peer.ip(), connect.udp_port);
                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

                    players.lock().expect("players lock poisoned").insert(player_id, Player {
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
                    udp_clients.lock().expect("udp_clients lock poisoned").insert(udp_addr, player_id);

                    if let Err(e) = stream.write_all(&Connected { player_id }.serialize()).await {
                        println!("[!] Failed to send legacy CONNECTED: {}", e);
                        players.lock().expect("players lock poisoned").remove(&player_id);
                        udp_clients.lock().expect("udp_clients lock poisoned").remove(&udp_addr);
                        return;
                    }
                    println!("[+] Legacy client {} connected as player {}", peer, player_id);

                    let (mut read_half, mut write_half) = stream.into_split();

                    // Write task: drain the mpsc channel to the TCP socket
                    tokio::spawn(async move {
                        while let Some(data) = rx.recv().await {
                            if write_half.write_all(&data).await.is_err() {
                                break;
                            }
                        }
                    });

                    // Read loop: dispatch reliable messages until disconnect
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

                    players.lock().expect("players lock poisoned").remove(&player_id);
                    udp_clients.lock().expect("udp_clients lock poisoned").remove(&udp_addr);
                    println!("[-] Legacy client {} (player {}) disconnected", peer, player_id);
                });
            }
            Err(e) => println!("[!] Legacy TCP accept error: {}", e),
        }
    }
}