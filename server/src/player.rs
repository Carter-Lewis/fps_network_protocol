use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use tokio::io::AsyncWriteExt;
use tokio::net::UdpSocket;
use protocol::PlayerInput;

pub struct Player {
    pub id: u16,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub health: i32,
    pub alive: bool,
    pub udp_addr: Option<SocketAddr>,
    pub tcp_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    pub wt_conn: Option<wtransport::Connection>,
}

impl Player {
    pub async fn send_reliable(&self, data: Vec<u8>) {
        if let Some(c) = &self.wt_conn {
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

    pub fn send_unreliable(&self, data: Vec<u8>, udp: &Arc<UdpSocket>) {
        if let Some(c) = &self.wt_conn {
            let _ = c.send_datagram(data);
        } else if let Some(addr) = self.udp_addr {
            let _ = udp.try_send_to(&data, addr);
        }
    }
}

pub type Players = Arc<Mutex<HashMap<u16, Player>>>;
pub type UdpClients = Arc<Mutex<HashMap<SocketAddr, u16>>>;

/// Applies a PlayerInput to a player's position and rotation.
/// Matches Godot's Y-rotation basis so movement feels consistent.
pub fn apply_movement(p: &mut Player, input: &PlayerInput) {
    let speed = 6.0; // matches Godot speed
    let delta = 1.0 / 60.0; // assumes ~60fps

    let input_x = input.move_x as f32;
    let input_z = input.move_z as f32;

    // Normalize input vector
    let len = (input_x * input_x + input_z * input_z).sqrt();
    let (dir_x, dir_z) = if len > 0.0 {
        (input_x / len, input_z / len)
    } else {
        (0.0, 0.0)
    };

    // Rotate by yaw — matches Godot's Y-rotation basis
    let yaw = input.yaw;
    let world_x = dir_x * yaw.cos() + dir_z * yaw.sin();
    let world_z = -dir_x * yaw.sin() + dir_z * yaw.cos();

    p.pos[0] += world_x * speed * delta;
    p.pos[2] += world_z * speed * delta;
    p.yaw = input.yaw;
    p.pitch = input.pitch;
    p.pos[1] = input.pos_y;
}