use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use protocol::*;
use crate::player::{Player, Players};
use crate::state::NEXT_PLAYER_ID;
use crate::game::{apply_player_input, handle_swing, handle_respawn, broadcast_player_left};

/// Handles a single WebTransport client connection.
/// Datagrams carry PlayerInput (unreliable).
/// Uni streams carry Connect, Swing, and RespawnRequest (reliable).
pub async fn handle_wt_client(conn: wtransport::Connection, players: Players, udp: Arc<UdpSocket>) {
    let _ = udp; // kept for API symmetry; WT clients use conn.send_datagram
    let mut my_id: Option<u16> = None;

    loop {
        tokio::select! {
            // Unreliable path: player movement input
            dgram = conn.receive_datagram() => {
                let Ok(d) = dgram else { break; };
                let bytes = d.payload();
                if bytes.is_empty() { continue; }
                match bytes[0] {
                    MSG_PLAYER_INPUT => apply_player_input(&players, &bytes),
                    _ => {}
                }
            }

            // Reliable path: connect, swing, respawn
            stream = conn.accept_uni() => {
                let Ok(mut s) = stream else { break; };
                let mut buf = Vec::new();
                // Ignore read errors; buf will contain whatever was received
                let _ = s.read_to_end(&mut buf).await;
                if buf.is_empty() { continue; }
                match buf[0] {
                    MSG_CONNECT => {
                        let pid = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed);
                        my_id = Some(pid);
                        players.lock().expect("players lock poisoned").insert(pid, Player {
                            id: pid,
                            pos: [0.0; 3],
                            yaw: 0.0,
                            pitch: 0.0,
                            health: 100,
                            alive: true,
                            udp_addr: None,
                            tcp_tx: None,
                            wt_conn: Some(conn.clone()),
                        });
                        let resp = Connected { player_id: pid }.serialize();
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
        players.lock().expect("players lock poisoned").remove(&pid);
        broadcast_player_left(&players, pid).await;
        println!("[-] WT player {pid} disconnected");
    }
}

/// Debug/test stub — logs incoming datagrams and streams without processing them.
#[allow(dead_code)]
async fn _client(conn: wtransport::Connection, _players: Players) {
    println!("[+] WT client connected: {}", conn.remote_address());
    loop {
        tokio::select! {
            dgram = conn.receive_datagram() => match dgram {
                Ok(d) => println!("[>] datagram: {} bytes", d.payload().len()),
                Err(e) => { println!("[-] dgram closed: {e}"); break; }
            },
            stream = conn.accept_uni() => match stream {
                Ok(_s) => println!("[>] uni stream"),
                Err(e) => { println!("[-] stream accept closed: {e}"); break; }
            }
        }
    }
}