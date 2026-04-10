use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

fn handle_client(mut stream: TcpStream) {
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
                let msg = String::from_utf8_lossy(&buf[..n]);
                println!("[>] Received from {}: {}", peer, msg.trim());

                // Echo it back with a prefix
                let response = format!("SERVER ECHO: {}", msg.trim());
                if stream.write_all(response.as_bytes()).is_err() {
                    break;
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

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| handle_client(stream));
            }
            Err(e) => {
                println!("[!] Connection error: {}", e);
            }
        }
    }
}