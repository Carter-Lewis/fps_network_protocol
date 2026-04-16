use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::thread;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Duration;

//game loop function
fn game_loop(players: Arc<Mutex<HashMap<u16, Player>>>) {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket for game loop");

    println!("[*] Game loop started! (~20hz)");

    loop{
        {
            let players_guard = players.lock().unwrap();
            if !players_guard.is_empty() {
                let player_count = players_guard.len() as u8;

                let mut packet = Vec::new();
                packet.push(0x11); //this is the world state
                packet.push(player_count);

                for player in players_guard.values() {
                    packet.extend_from_slice(&player.id.to_be_bytes());
                    packet.extend_from_slice(&0.0f32.to_be_bytes()); //pox
                    packet.extend_from_slice(&0.0f32.to_be_bytes()); //posy
                    packet.extend_from_slice(&0.0f32.to_be_bytes()); //posz
                    packet.extend_from_slice(&0.0f32.to_be_bytes()); //yaw
                    packet.extend_from_slice(&0.0f32.to_be_bytes()); //pitch
                }

                for player in players_guard.values() {
                    let addr = format!("{}:{}", player.ip, player.udp_port);

                    match socket.send_to(&packet, &addr) {
                        Ok(bytes_sent) => {
                            println!("[LOOP] Sent WorldState ({} bytes) to {}", bytes_sent, addr);
                        }
                        Err(e) => {
                            println!("[LOOP] Failed to send WorldState to {}: {}", addr, e);
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

//player struct
#[derive(Debug, Clone)]
struct Player {
    id: u16,
    ip: String,
    udp_port: u16,
}

//udp listener function
fn udp_listener() {
    let socket = UdpSocket::bind("0.0.0.0:7778")
        .expect("Failed to bind UDP socket on port 7778");

    println!("[*] UDP listener running on 0.0.0.0:7778");

    let mut buf = [0u8; 512];

    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, src_addr)) => {
                println!("[UDP] Received {} bytes from {}", n, src_addr);
                println!("[UDP] Raw bytes: {:?}", &buf[..n]);

                if n == 0 {
                    continue;
                }

                let msg_type = buf[0];
                println!("[UDP] MsgType: {:#04x}", msg_type);

                match msg_type {
                    0x02 => {
                        if n < 13 {
                            println!("[UDP] PlayerInput packet too short");
                            continue;
                        }

                        let seq_num = u16::from_be_bytes([buf[1], buf[2]]);
                        let yaw = f32::from_be_bytes([buf[3], buf[4], buf[5], buf[6]]);
                        let pitch = f32::from_be_bytes([buf[7], buf[8], buf[9], buf[10]]);
                        let move_x = buf[11] as i8;
                        let move_z = buf[12] as i8;

                        println!("[UDP] Received PLAYER INPUT message");
                        println!("[UDP] SeqNum: {}", seq_num);
                        println!("[UDP] Yaw: {}", yaw);
                        println!("[UDP] Pitch: {}", pitch);
                        println!("[UDP] MoveX: {}", move_x);
                        println!("[UDP] MoveZ: {}", move_z);
                    }
                    0x11 => {
                        println!("[UDP] Received WORLD STATE message");
                    }
                    _ => {
                        println!("[UDP] Unknown message type");
                    }
                }
            }
            Err(e) => {
                println!("[UDP] Error receiving packet: {}", e);
            }
        }
    }
}

fn handle_client(mut stream: TcpStream, next_player_id: Arc<Mutex<u16>>, players: Arc<Mutex<HashMap<u16, Player>>>) {
    let peer = stream.peer_addr().unwrap();
    println!("[+] Client connected: {}", peer);

    let mut buf = [0u8; 512];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => {
                println!("[-] Client disconnected: {}", peer);
                break;
            }
            Ok(n) => {
                println!("[>] Received {} bytes from {}", n, peer);
                println!("Raw bytes: {:?}", &buf[..n]);

                if n == 0 {
                    continue;
                }

                let msg_type = buf[0];
                println!("MsgType: {:#04x}", msg_type);

                //packet router
                match msg_type {
                    //connect
                    0x01 => {
                        if n < 3 {
                            println!("[!] Connect packet too short");
                            continue;
                        }

                        let udp_port = u16::from_be_bytes([buf[1], buf[2]]);
                        println!("[+] Received CONNECT message");
                        println!("[+] Client says its UDP port is {}", udp_port);

                        //tracks and assigns player ids
                        let player_id = {
                            let mut guard = next_player_id.lock().unwrap();
                            let id = *guard;
                            *guard += 1;
                            id
                        };

                        //store player
                        let player = Player {
                            id: player_id,
                            ip: peer.ip().to_string(),
                            udp_port: udp_port,
                        };

                        //add player to hash map
                        {
                            let mut players_guard = players.lock().unwrap();
                            players_guard.insert(player_id, player);
                            println!("[+] Stored player {}", player_id);
                            println!("[+] Connected players now: {:?}", players_guard);
                        }

                        //server sends response
                        let id_bytes = player_id.to_be_bytes();
                        let response = [0x10, id_bytes[0], id_bytes[1]];

                        if let Err(e) = stream.write_all(&response) {
                            println!("[!] Failed to send Connected response to {} : {}", peer, e);
                            break;
                        }
                        println!("[+] Sent CONNECTED message with Player ID {}", player_id);
                    }
                    //player input
                    0x02 => {
                        println!("[+] Received PLAYER INPUT message");
                    }
                    _ => {
                        println!("[-] Unknown message type");
                    }
                }
            }
            Err(e) => {
                println!("[!] Error reading from {}: {}", peer, e);
                break;
            }
        }
    }
}


fn main() {
    let port = std::env::var("GAME_PORT").unwrap_or_else(|_| "7777".to_string());
    let addr = format!("0.0.0.0:{}", port);

    let listener = TcpListener::bind(&addr).expect("Failed to bind to address");
    println!("[*] Game server listening on {}", addr);

    //spawns udp listener
    thread::spawn(|| udp_listener());

    let next_player_id = Arc::new(Mutex::new(1u16));
    let players = Arc::new(Mutex::new(HashMap::<u16, Player>::new()));

    //spawns game loop
    let players_for_loop = Arc::clone(&players);
    thread::spawn(move || game_loop(players_for_loop));

    //accepts tcp connections indefinitely
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let next_player_id_clone = Arc::clone(&next_player_id);
                let players_clone = Arc::clone(&players);
                thread::spawn(move || handle_client(stream, next_player_id_clone, players_clone));
            }
            Err(e) => {
                println!("[!] Connection error: {}", e);
            }
        }
    }
}