use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use protocol::*;
use crate::player::{Players, apply_movement};
use crate::state::WORLD_TICK;

/// Serializes the full world state for broadcast.
pub fn snapshot_world(players: &Players) -> Vec<u8> {
    let players = players.lock().expect("players lock poisoned");
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

/// Broadcasts a world snapshot to all WebTransport clients at ~60Hz.
/// Legacy UDP clients are handled by their own broadcast in legacy.rs.
pub async fn broadcast_loop(players: Players) {
    let mut tick = tokio::time::interval(Duration::from_millis(16));
    loop {
        tick.tick().await;
        let snapshot = snapshot_world(&players);
        let players_g = players.lock().expect("players lock poisoned");
        for p in players_g.values() {
            if let Some(c) = &p.wt_conn {
                let _ = c.send_datagram(snapshot.clone());
            }
        }
    }
}

/// Applies a UDP PlayerInput datagram to the named player's position.
pub fn apply_player_input(players: &Players, bytes: &[u8]) {
    if let Some(input) = PlayerInput::deserialize(bytes) {
        let mut players = players.lock().expect("players lock poisoned");
        if let Some(p) = players.get_mut(&input.player_id) {
            apply_movement(p, &input);
        }
    }
}

/// Handles a swing event: deal 25 damage to all players within 2 units,
/// send health updates, send YouDied on kills, and notify all players
/// of the swing animation.
pub async fn handle_swing(players: &Players, buf: &[u8]) {
    let Some(swing) = Swing::deserialize(buf) else { return; };
    let notify = SwingNotify { player_id: swing.player_id }.serialize();

    // Collect victim IDs without holding the lock across awaits
    let victims: Vec<u16> = {
        let players_g = players.lock().expect("players lock poisoned");
        let Some(sp) = players_g.get(&swing.player_id).map(|p| p.pos) else { return; };
        players_g.values()
            .filter(|p| p.id != swing.player_id && p.alive)
            .filter(|p| {
                let dx = p.pos[0] - sp[0];
                let dy = p.pos[1] - sp[1];
                let dz = p.pos[2] - sp[2];
                (dx * dx + dy * dy + dz * dz).sqrt() < 2.0
            })
            .map(|p| p.id)
            .collect()
    };

    for victim_id in victims {
        let (new_health, died) = {
            let mut players_g = players.lock().expect("players lock poisoned");
            if let Some(p) = players_g.get_mut(&victim_id) {
                p.health -= 25;
                if p.health <= 0 {
                    p.alive = false;
                }
                (p.health, p.health <= 0)
            } else {
                continue;
            }
        };

        // Clone connection handles before releasing the lock
        let (victim_wt, victim_tcp) = {
            let players_g = players.lock().expect("players lock poisoned");
            players_g
                .get(&victim_id)
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

    // Broadcast swing animation to all WT clients
    let players_g = players.lock().expect("players lock poisoned");
    for p in players_g.values() {
        if let Some(c) = &p.wt_conn {
            let _ = c.send_datagram(notify.clone());
        }
    }
}

/// Resets the named player to full health and origin position.
pub fn handle_respawn(players: &Players, buf: &[u8]) {
    let Some(req) = RespawnRequest::deserialize(buf) else { return; };
    let mut players_g = players.lock().expect("players lock poisoned");
    if let Some(p) = players_g.get_mut(&req.player_id) {
        p.health = 100;
        p.alive = true;
        p.pos = [0.0, 0.0, 0.0];
    }
}

/// Sends a fresh world snapshot to all WT clients after a player disconnects.
pub async fn broadcast_player_left(players: &Players, _pid: u16) {
    let snapshot = snapshot_world(players);
    let players_g = players.lock().expect("players lock poisoned");
    for p in players_g.values() {
        if let Some(c) = &p.wt_conn {
            let _ = c.send_datagram(snapshot.clone());
        }
    }
}