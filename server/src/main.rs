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
    health: i32,
    alive: bool,
    udp_addr: Option<SocketAddr>,
    tcp_stream: Option<Arc<Mutex<TcpStream>>>,
}

// Shared game state
type Players = Arc<Mutex<HashMap<u16, Player>>>;

fn handle_client(mut stream: TcpStream, players: Players, udp: Arc<UdpSocket>) {
    let peer = stream.peer_addr().unwrap();
    println!("[+] Client connected: {}", peer);

    let mut buf = [0u8; 512];
    let mut my_player_id: Option<u16> = None;

    // loop to keep connection alive and detect disconnect
    loop {
        match stream.read(&mut buf) {
            Ok(0) => {
                println!("[-] Client disconnected: {}", peer);
                break;
            }
            Ok(n) => {
                let msg_type = buf[0];

                if msg_type == MSG_CONNECT {
                    if let Some(connect) = Connect::deserialize(&buf[..n]) {
                        let player_id = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed);

                        println!(
                            "[+] Assigned PlayerID {} (client UDP port {})",
                            player_id, connect.udp_port
                        );

                        my_player_id = Some(player_id);

                        let mut players = players.lock().unwrap();
                        players.insert(player_id, Player {
                            id: player_id,
                            pos: [0.0, 0.0, 0.0],
                            yaw: 0.0,
                            pitch: 0.0,
                            health: 100,
                            alive: true,
                            udp_addr: None,
                            tcp_stream: Some(Arc::new(Mutex::new(stream.try_clone().unwrap()))),
                        });

                        let response = Connected { player_id };
                        if let Err(e) = stream.write_all(&response.serialize()) {
                            println!("[!] Failed to send Connected response: {}", e);
                            break;
                        }
                        println!("[+] Sent CONNECTED with Player ID {}", player_id);
                    }
                } else if msg_type == MSG_SWING {
                    if let Some(swing) = Swing::deserialize(&buf[..n]) {
                        let hits: Vec<u16> = {
                            let players_guard = players.lock().unwrap();

                            // Clean up the hit logic a bit
                            if let Some(attacker) = players_guard.get(&swing.player_id) {
                                let attacker_pos = attacker.pos;
                                let attacker_yaw = attacker.yaw;

                                let fwd_x = attacker_yaw.sin();
                                let fwd_z = -attacker_yaw.cos();

                                let mut hits = Vec::new();
                                for target in players_guard.values() {
                                    if target.id == swing.player_id { continue; }
                                    if !target.alive { continue; }

                                    let dx = target.pos[0] - attacker_pos[0];
                                    let dy = target.pos[1] - attacker_pos[1];
                                    let dz = target.pos[2] - attacker_pos[2];
                                    let dist = (dx*dx + dy*dy + dz*dz).sqrt();

                                    if dist <= 3.5 {
                                        let horiz_dist = (dx*dx + dz*dz).sqrt();
                                        if horiz_dist < 0.001 { continue; }
                                        let dot = (dx / horiz_dist) * fwd_x + (dz / horiz_dist) * fwd_z;
                                        if dot > 0.25 {
                                            hits.push(target.id);
                                            println!("[!] Player {} hit player {} (dist {:.2}, dot {:.2})", swing.player_id, target.id, dist, dot);
                                        }
                                    }
                                }
                                hits
                            } else {
                                Vec::new()
                            }
                        };

                        // Apply damage
                        for hit_id in hits {
                            let mut players_guard = players.lock().unwrap();
                            if let Some(target) = players_guard.get_mut(&hit_id) {
                                target.health -= 20;
                                let new_health = target.health;
                                let alive = new_health > 0;
                                target.alive = alive;

                                // Send health update to hit player
                                if let Some(tcp) = &target.tcp_stream {
                                    let msg = HealthUpdate { player_id: hit_id, health: new_health };
                                    let _ = tcp.lock().unwrap().write_all(&msg.serialize());
                                }

                                // send death notice if health hits 0
                                if !alive {
                                    if let Some(tcp) = &target.tcp_stream {
                                        let msg = YouDied { player_id: hit_id };
                                        let _ = tcp.lock().unwrap().write_all(&msg.serialize());
                                    }
                                    println!("[!] Player {} died", hit_id);
                                }
                            }
                        }

                        // notify all other players of swing for animation
                        let notify = SwingNotify { player_id: swing.player_id };
                        let notify_bytes = notify.serialize();
                        let players_guard = players.lock().unwrap();
                        for other in players_guard.values() {
                            if other.id != swing.player_id {
                                if let Some(addr) = other.udp_addr {
                                    let _ = udp.send_to(&notify_bytes, addr);
                                }
                            }
                        }
                    }
                } else if msg_type == MSG_RESPAWN_REQUEST {
                    if let Some(req) = RespawnRequest::deserialize(&buf[..n]) {
                        println!("[+] Respawn request received for player {}", req.player_id);
                        let mut players = players.lock().unwrap();
                        if let Some(p) = players.get_mut(&req.player_id) {
                            p.health = 100;
                            p.alive = true;
                            // Make people respawn randomly within the bounds
                            let angle = rand::random::<f32>() * std::f32::consts::TAU;
                            let radius = rand::random::<f32>() * 8.0;
                            p.pos = [angle.cos() * radius, 0.0, angle.sin() * radius];
                            println!("[+] Player {} respawned at {:?}", req.player_id, p.pos);
                        }
                    }
                }
            }
            Err(e) => {
                println!("[!] Error reading from {}: {}", peer, e);
                break;
            }
        }
    }

    // cleanup after disconnect
    if let Some(id) = my_player_id {
        let mut players_guard = match players.lock() {
            Ok(guard) => guard,
            Err(e) => {
                println!("[!] Failed to lock players on disconnect: {}", e);
                return;
            }
        };

        players_guard.remove(&id);
        println!("[-] Removed player {} from HashMap. Players remaining: {}", id, players_guard.len());

        // reuse shared UDP socket for disconnect notify
        let socket = &udp;

        // build player left packet (0x12 + departed player id)
        let mut notify = Vec::new();
        notify.push(0x12u8);
        notify.extend_from_slice(&id.to_be_bytes());

        // notify remaining players, log and continue if one fails
        for player in players_guard.values() {
            if let Some(addr) = player.udp_addr {
                match socket.send_to(&notify, addr) {
                    Ok(_) => println!("[-] Notified player {} of disconnect", player.id),
                    Err(e) => println!("[!] Failed to notify player {} of disconnect: {}", player.id, e),
                }
            }
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
    let udp_socket = Arc::new(UdpSocket::bind("0.0.0.0:7778").expect("Failed to bind UDP socket"));
    udp_socket.set_nonblocking(true).unwrap();
    println!("[*] UDP listening on port 7778");

    // Thread to handle UDP input
    // get PlayerInput packets and update player positions
    let players_clone = players.clone();
    let udp_socket_clone = Arc::clone(&udp_socket);

    thread::spawn(move || {
        let mut buf = [0u8; 1024];
        loop {
            if let Ok((len, addr)) = udp_socket_clone.recv_from(&mut buf) {
                // Check message type
                if buf[0] == MSG_PLAYER_INPUT {
                    if let Some(input) = PlayerInput::deserialize(&buf[..len]) {
                        let mut players = players_clone.lock().unwrap();

                        // Find player using UDP addresssd
                        if let Some(p) = players.get_mut(&input.player_id) {
                            if p.udp_addr.is_none() {
                                p.udp_addr = Some(addr);
                                println!("[+] Bound UDP {} to Player {}", addr, p.id);
                            }
                            if p.alive {
                                let speed = 0.1;
                                let yaw = p.yaw;
                                p.pos[0] += (input.move_x as f32 * yaw.cos() + input.move_z as f32 * yaw.sin()) * speed;
                                p.pos[2] += (input.move_z as f32 * yaw.cos() - input.move_x as f32 * yaw.sin()) * speed;
                                p.pos[1] = input.pos_y;
                                p.yaw = input.yaw;
                                p.pitch = input.pitch;

                                // Clamping will force the value to stay within this range to ensure
                                // our server doesn't tell clients they can leave the wall bounds
                                p.pos[0] = p.pos[0].clamp(-19.0, 19.0);
                                p.pos[2] = p.pos[2].clamp(-19.0, 19.0);
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
    let udp_socket_clone = Arc::clone(&udp_socket);

    thread::spawn(move || {
        loop {
            let players = players_clone.lock().unwrap();

            // Build world state from current data
            let world = WorldState {
                players: players.values()
                    .filter(|p| p.udp_addr.is_some() && p.alive)
                    .map(|p| PlayerState {
                        player_id: p.id,

                        // Actual positions
                        pos_x: p.pos[0],
                        pos_y: p.pos[1],
                        pos_z: p.pos[2],

                        yaw: p.yaw,
                        pitch: p.pitch,
                        health: p.health,
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

    for stream in tcp_listener.incoming() {
        match stream {
            Ok(stream) => {
                let players_clone = players.clone();
                let udp_clone = Arc::clone(&udp_socket);
                thread::spawn(move || handle_client(stream, players_clone, udp_clone));
            }
            Err(e) => {
                println!("[!] Connection error: {}", e);
            }
        }
    }
}