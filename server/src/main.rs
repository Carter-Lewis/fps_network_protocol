mod cert;
mod game;
mod legacy;
mod player;
mod state;
mod wt;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;
use player::Players;

#[tokio::main]
async fn main() {
    let players: Players = Arc::new(Mutex::new(HashMap::new()));

    tokio::spawn(legacy::run_legacy_tcp_udp(players.clone(), Default::default()));
    tokio::spawn(game::broadcast_loop(players.clone()));

    let udp_wt = Arc::new(
        UdpSocket::bind("0.0.0.0:0")
            .await
            .expect("failed to bind WebTransport UDP socket"),
    );

    let endpoint = cert::build_endpoint().await;
    println!("[*] WebTransport server listening on UDP 7777");

    loop {
        let incoming = endpoint.accept().await;
        let session_request = match incoming.await {
            Ok(req) => req,
            Err(e) => { println!("[!] handshake failed: {e}"); continue; }
        };
        println!(
            "[+] WT request: path={} authority={}",
            session_request.path(),
            session_request.authority()
        );
        let connection = match session_request.accept().await {
            Ok(c) => c,
            Err(e) => { println!("[!] WT accept failed: {e}"); continue; }
        };
        tokio::spawn(wt::handle_wt_client(connection, players.clone(), udp_wt.clone()));
    }
}