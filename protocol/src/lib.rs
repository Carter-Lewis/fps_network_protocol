// Crate defines the binary networking protocol between client and server
// Defines message formats
// Serialize
// Deserialize

// Message type identifiers
pub const MSG_CONNECT: u8 = 0x01; // Client -> Server (TCP)
pub const MSG_CONNECTED: u8 = 0x10; // Server -> Client (TCP)
pub const MSG_PLAYER_INPUT: u8 = 0x02; // Client -> Server (UDP)
pub const MSG_WORLD_STATE: u8 = 0x11; // Server -> Client (UDP)


// Connect message sent by client when first joins server
// Tells server which UDP port the client will listen on
// Allows server to send UDP game updates back
#[derive(Debug, PartialEq)]
pub struct Connect {
    pub udp_port: u16,
}

impl Connect {
    // Serialize (struct -> bytes)
    // [0] = MSG_CONNECT
    // [1-2] = udp_port
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(3);
        buf.push(MSG_CONNECT);
        buf.extend(&self.udp_port.to_be_bytes());
        buf
    }

    //Deserialize
    pub fn deserialize(buf: &[u8]) -> Option<Self> {
        if buf.len() < 3 {
            return None;
        }
        if buf[0] != MSG_CONNECT {
            return None;
        }
        let udp_port = u16::from_be_bytes([buf[1], buf[2]]);

        Some(Self { udp_port })
    }
}



// Connected message sent by server after accepting a connection
// Confirms client is registered
// Assigns a unique PlayerID used in all future packets
#[derive(Debug, PartialEq)]
pub struct Connected {
    pub player_id: u16,
}

impl Connected {
    // Serialize
    // [0] = MSG_CONNECTED
    // [1-2] = player_id
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(3);
        buf.push(MSG_CONNECTED);
        buf.extend(&self.player_id.to_be_bytes());
        buf
    }

    //Deserialize
    pub fn deserialize(buf: &[u8]) -> Option<Self> {
        if buf.len() < 3 {
            return None;
        }
        if buf[0] != MSG_CONNECTED {
            return None;
        }
        let player_id = u16::from_be_bytes([buf[1], buf[2]]);

        Some(Self { player_id })
    }
}



// Transmit player movement input
// Send camera orientation (yaw/pitch)
// Seq number for ordering
#[derive(Debug, PartialEq)]
pub struct PlayerInput {
    pub seq_num: u16,
    pub yaw: f32, // Camera horizontal rotation (in radians)
    pub pitch: f32, // Camera vertical rotation (in radians)
    pub move_x: i8,
    pub move_z: i8,
}

impl PlayerInput {
    // [0] = MsgType
    // [1-2] = SeqNum
    // [3-6] = Yaw
    // [7-10] = Pitch
    // [11] = MoveX
    // [12] = MoveZ
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(13);
        buf.push(MSG_PLAYER_INPUT);

        buf.extend(&self.seq_num.to_be_bytes());

        buf.extend(&self.yaw.to_be_bytes());
        buf.extend(&self.pitch.to_be_bytes());

        buf.push(self.move_x as u8);
        buf.push(self.move_z as u8);

        buf
    }

    pub fn deserialize(buf: &[u8]) -> Option<Self> {
        // Make sure packet is big enough
        if buf.len() < 13 {
            return None;
        }
        // Validate message type
        if buf[0] != MSG_PLAYER_INPUT {
            return None;
        }
        let seq_num = u16::from_be_bytes([buf[1], buf[2]]);
        let yaw = f32::from_be_bytes(buf[3..7].try_into().ok()?);
        let pitch = f32::from_be_bytes(buf[7..11].try_into().ok()?);
        let move_x = buf[11] as i8;
        let move_z = buf[12] as i8;

        Some(Self { seq_num, yaw, pitch, move_x, move_z })
    }
}


// World State message
// Give full state of the world
// Include all connected players
// Used to update game world

// Player State used to help
#[derive(Debug, PartialEq)]
pub struct PlayerState{
    pub player_id: u16,

    // Position in 3D world space
    pub pos_x: f32,
    pub pos_y: f32,
    pub pos_z: f32,

    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Debug, PartialEq)]
pub struct WorldState {
    pub players: Vec<PlayerState>,
}

impl WorldState {
    // [0] = MsgType
    // [1] = PlayerCount
    // For each player:
    // [0-1] = PlayerID
    // [2-5] = PosX
    // [6-9] = PosY
    // [10-13] = PosZ
    // [14-17] = Yaw
    // [18-21] = Pitch

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(MSG_WORLD_STATE);
        buf.push(self.players.len() as u8);

        // Serialize each player in order
        for p in &self.players {
            buf.extend(&p.player_id.to_be_bytes());

            buf.extend(&p.pos_x.to_be_bytes());
            buf.extend(&p.pos_y.to_be_bytes());
            buf.extend(&p.pos_z.to_be_bytes());

            buf.extend(&p.yaw.to_be_bytes());
            buf.extend(&p.pitch.to_be_bytes());
        }
        buf
    }

    pub fn deserialize(buf: &[u8]) -> Option<Self> {
        // Has to have MsgType and PlayerCount
        if buf.len() < 2 {
            return None;
        }
        // Validate message type
        if buf[0] != MSG_WORLD_STATE {
            return None;
        }

        let player_count = buf[1] as usize;

        // Each player takes 22 bytes
        let expected_size = 2 + player_count * 22;

        // Make sure buffer is big enough
        if buf.len() != expected_size {
            return None;
        }

        let mut players = Vec::with_capacity(player_count);

        // Start read after header
        let mut offset = 2;

        for _ in 0..player_count {
            let player_id = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
            offset += 2;

            let pos_x = f32::from_be_bytes(buf[offset..offset + 4].try_into().ok()?);
            offset += 4;

            let pos_y = f32::from_be_bytes(buf[offset..offset + 4].try_into().ok()?);
            offset += 4;

            let pos_z = f32::from_be_bytes(buf[offset..offset + 4].try_into().ok()?);
            offset += 4;

            let yaw = f32::from_be_bytes(buf[offset..offset + 4].try_into().ok()?);
            offset += 4;

            let pitch = f32::from_be_bytes(buf[offset..offset + 4].try_into().ok()?);
            offset += 4;

            players.push(PlayerState {
                player_id,
                pos_x,
                pos_y,
                pos_z,
                yaw,
                pitch,
            });
        }
        Some(Self { players })
    }
}




// Unit Tests
#[cfg(test)]
mod tests {
    use super::*;

    // Connect round-trip test
    #[test]
    fn connect_roundtrip() {
        let msg = Connect { udp_port: 7778 };
        let bytes = msg.serialize();
        let decoded = Connect::deserialize(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    // Connected round-trip test
    #[test]
    fn connected_roundtrip() {
        let msg = Connected { player_id: 42 };
        let bytes = msg.serialize();
        let decoded = Connected::deserialize(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    // Wrong message type test
    #[test]
    fn connect_reject_wrong_type() {
        let bad = vec![0xFF, 0x00, 0x00];
        assert!(Connect::deserialize(&bad).is_none());
    }
    #[test]
    fn connected_reject_wrong_type(){
        let bad = vec![0xFF, 0x00, 0x00];
        assert!(Connected::deserialize(&bad).is_none());
    }

    // Player input round-trip test
    #[test]
    fn player_input_roundtrip() {
        let msg = PlayerInput {
            seq_num: 42,
            yaw: 1.2,
            pitch: -0.7,
            move_x: 1,
            move_z: -1,
        };

        let bytes = msg.serialize();
        let decoded = PlayerInput::deserialize(&bytes).unwrap();

        assert_eq!(decoded.seq_num, msg.seq_num);
        assert_eq!(decoded.yaw, msg.yaw);
        assert_eq!(decoded.pitch, msg.pitch);
        assert_eq!(decoded.move_x, msg.move_x);
        assert_eq!(decoded.move_z, msg.move_z);
    }

    // World State round-trip test
    #[test]
    fn world_state_roundtrip() {
        let msg = WorldState {
            players: vec![
                PlayerState {
                    player_id: 1,
                    pos_x: 1.0,
                    pos_y: 2.0,
                    pos_z: 3.0,
                    yaw: 0.5,
                    pitch: -0.5,
                },
                PlayerState {
                    player_id: 2,
                    pos_x: 4.0,
                    pos_y: 5.0,
                    pos_z: 6.0,
                    yaw: 1.5,
                    pitch: -1.0,
                },
            ],
        };

        let bytes = msg.serialize();
        let decoded = WorldState::deserialize(&bytes).unwrap();

        assert_eq!(decoded, msg);
    }

    // World State bad size test
    #[test]
    fn world_state_reject_bad_size(){
        let bad = vec![MSG_WORLD_STATE, 2];
        assert!(WorldState::deserialize(&bad).is_none());
    }
}
