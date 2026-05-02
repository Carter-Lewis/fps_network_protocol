use std::error::Error;
use std::net::{TcpStream, UdpSocket};
use std::time::Duration;

use protocol::{Connect, Connected, MSG_CONNECTED, MSG_WORLD_STATE};

fn main() -> Result<(), Box<dyn Error>> {
    let udp_socket = UdpSocket::bind("0.0.0.0:0")?;
    udp_socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    let udp_port = udp_socket.local_addr()?.port();

    let mut tcp_stream = TcpStream::connect("127.0.0.1:7777")?;
    tcp_stream.set_nodelay(true)?;

    let connect = Connect { udp_port };
    std::io::Write::write_all(&mut tcp_stream, &connect.serialize())?;
    println!("[legacy_smoke] Sent TCP CONNECT with udp_port={}", udp_port);

    let mut response = [0u8; 3];
    std::io::Read::read_exact(&mut tcp_stream, &mut response)?;
    if response[0] != MSG_CONNECTED {
        return Err(format!("unexpected TCP response type: {}", response[0]).into());
    }

    let connected = Connected::deserialize(&response).ok_or("failed to parse CONNECTED")?;
    println!("[legacy_smoke] Received CONNECTED player_id={}", connected.player_id);

    // Send one player input packet so we know the UDP path is wired up too.
    let mut packet = Vec::new();
    packet.push(protocol::MSG_PLAYER_INPUT);
    packet.extend(&connected.player_id.to_be_bytes());
    packet.extend(&1u16.to_be_bytes());
    packet.extend(&0.0f32.to_be_bytes());
    packet.extend(&0.0f32.to_be_bytes());
    packet.push(0);
    packet.push(0);
    packet.extend(&0.0f32.to_be_bytes());
    packet.push(0);
    udp_socket.send_to(&packet, "127.0.0.1:7778")?;
    println!("[legacy_smoke] Sent UDP player input");

    let mut buf = [0u8; 1500];
    match udp_socket.recv_from(&mut buf) {
        Ok((n, _)) => {
            if n > 0 && buf[0] == MSG_WORLD_STATE {
                println!("[legacy_smoke] Received world state datagram ({} bytes)", n);
            } else {
                println!("[legacy_smoke] Received UDP datagram ({} bytes, type={})", n, buf[0]);
            }
        }
        Err(e) => {
            println!("[legacy_smoke] No UDP response within timeout: {}", e);
        }
    }

    Ok(())
}