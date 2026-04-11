// Crate defines the binary networking protocol between client and server
// Defines message formats
// Serialize
// Deserialize

// Message type identifiers
pub const MSG_CONNECT: u8 = 0x01; // Client -> Server (TCP)
pub const MSG_CONNECTED: u8 = 0x10; // Server -> Client (TCP)

// Connect message sent by client when first joins server
// Tells server which UDP port the client will listen on
// Allows server to send UDP game updates back
#[derive(Debug, PartialEq)]
pub struct Connect {
    pub udp_port: u16,
}

impl Connect {
    // Serialize (struct -> bytes)
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
    //Serialize
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(3);
        buf.push(MSG_CONNECT);
        buf.extend(&self.player_id.to_be_bytes());
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
        let player_id = u16::from_be_bytes([buf[1], buf[2]]);

        Some(Self { player_id })
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
}