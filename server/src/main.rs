use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket, SocketAddr};
use std::thread;

use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::sync::atomic::AtomicU16;
use protocol::*;
use std::time::Duration;
use std::sync::atomic::Ordering;

// Global PlayerID counter
static NEXT_PLAYER_ID: AtomicU16 = AtomicU16::new(1);

// Player struct
struct Player {
    id: u16,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    udp_addr: Option<SocketAddr>,
}

// Shared game state
type Players = Arc<Mutex<HashMap<u16, Player>>>;

fn handle_client(mut stream: TcpStream, players: Players) {
    let peer = stream.peer_addr().unwrap();
    println!("[+] Client connected: {}", peer);

    let mut buf = [0u8; 512];

    match stream.read(&mut buf) {
        Ok(0) => {
            println!("[-] Client disconnected: {}", peer);
        }
        Ok(n) => {
            // First byte tells message type
            let msg_type = buf[0];

            if msg_type == MSG_CONNECT {
                if let Some(connect) = Connect::deserialize(&buf[..n]) {
                    let player_id = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed);

                    println!(
                        "[+] Assigned PlayerID {} (client UDP port {})",
                        player_id, connect.udp_port
                    );

                    // Add player to shared state
                    let mut players = players.lock().unwrap();

                    players.insert(player_id, Player {
                        id: player_id,

                        // Start at origin
                        pos: [0.0, 0.0, 0.0],
                        yaw: 0.0,
                        pitch: 0.0,
                        udp_addr: None,
                    });

                    // Send connected response
                    let response = Connected { player_id };
                    stream.write_all(&response.serialize()).unwrap();
                }
            }
        }
        // Ok(n) => {
            // println!("[-] TCP client disconnected: {}", peer);
            // let msg = String::from_utf8_lossy(&buf[..n]);
            // println!("[>] Received from {}: {}", peer, msg.trim());

            // Echo it back with a prefix
            // let response = format!("SERVER ECHO: {}", msg.trim());
            // if stream.write_all(response.as_bytes()).is_err() {
                // break;
            // }
        // }
        Err(e) => {
            println!("[!] Error reading from {}: {}", peer, e);
            // break;
        }
    }

}


fn main() {
    // Shared player state
    let players: Players = Arc::new(Mutex::new(HashMap::new()));

    // TCP listener (connections)
    let tcp_listener = TcpListener::bind("0.0.0.0:7777").expect("Failed to bind TCP socket");
    println!("[*] TCP listening on port 7777");

    // UDP socket (game data)
    let udp_socket = UdpSocket::bind("0.0.0.0:7778").expect("Failed to bind UDP socket");

    udp_socket.set_nonblocking(true).unwrap();
    println!("[*] UDP listening on port 7778");

    // Thread to handle UDP input
    // get PlayerInput packets and update player positions
    let players_clone = players.clone();
    let udp_socket_clone = udp_socket.try_clone().unwrap();

    thread::spawn(move || {
        let mut buf = [0u8; 1024];
        loop {
            if let Ok((len, addr)) = udp_socket_clone.recv_from(&mut buf) {
                // Check message type
                if buf[0] == MSG_PLAYER_INPUT {
                    if let Some(input) = PlayerInput::deserialize(&buf[..len]) {
                        let mut players = players_clone.lock().unwrap();

                        // Find player using UDP address
                        let player = players.values_mut().find(|p| p.udp_addr == Some(addr));
                        if let Some(p) = player {
                            // Movement is based on client input, server computes real position
                            let speed = 0.1;

                            p.pos[0] += input.move_x as f32 * speed;
                            p.pos[2] += input.move_z as f32 * speed;

                            // Update camera
                            p.yaw = input.yaw;
                            p.pitch = input.pitch;

                            println!("[>] Player {} moved to {:?}", p.id, p.pos);
                        } else {
                            // UDP address first time
                            // Bind to player
                            if let Some(p) = players.values_mut().find(|p| p.udp_addr.is_none()) {
                                p.udp_addr = Some(addr);
                                println!("[+] Bound UDP {} to Player {}", addr, p.id);
                            }
                        }
                    }
                }
            }
        }
    });

    // Thread to broadcast world state
    // Sends full game state to all players
    let players_clone = players.clone();
    let udp_socket_clone = udp_socket.try_clone().unwrap();

    thread::spawn(move || {
        loop {
            let players = players_clone.lock().unwrap();

            // Build world state from current data
            let world = WorldState {
                players: players.values().map(|p| PlayerState {
                    player_id: p.id,

                    // Actual positions
                    pos_x: p.pos[0],
                    pos_y: p.pos[1],
                    pos_z: p.pos[2],

                    yaw: p.yaw,
                    pitch: p.pitch,
                }).collect(),
            };

            let bytes = world.serialize();

            // Send to all connected players
            for p in players.values() {
                if let Some(addr) = p.udp_addr {
                    let _ = udp_socket_clone.send_to(&bytes, addr);
                }
            }
            drop(players);

            // Can change speed of update if needed
            thread::sleep(Duration::from_millis(50));
        }
    });

    // let port = std::env::var("GAME_PORT").unwrap_or_else(|_| "7777".to_string());
    // let addr = format!("0.0.0.0:{}", port);

    // let listener = TcpListener::bind(&addr).expect("Failed to bind to address");
    // println!("[*] Game server listening on {}", addr);

    for stream in tcp_listener.incoming() {
        match stream {
            Ok(stream) => {
                let players_clone = players.clone();

                thread::spawn(move || handle_client(stream, players_clone));
            }
            Err(e) => {
                println!("[!] Connection error: {}", e);
            }
        }
    }
}