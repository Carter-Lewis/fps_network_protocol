# Multiplayer FPS — CSI 4321/5321 Networking Final Project

A real-time multiplayer first-person shooter built from scratch as a networking course project. Players can connect from a native desktop client or a web browser and fight each other in a shared 3D arena. All game logic runs on an authoritative Rust server; clients send input and receive world state updates.

---

## What is the project?

A multiplayer FPS game that demonstrates applied network programming concepts: custom binary protocol design, dual-transport support (WebTransport/QUIC and TCP+UDP), authoritative server-side game logic, and real-time position reconciliation between client and server.

The stack is:
- **Game client**: Godot 4 (GDScript), exportable to both native desktop and browser (HTML5)
- **Game server**: Rust with Tokio async runtime
- **Protocol library**: shared Rust crate with hand-written binary serialization

---

## Novel Work

- **Dual-transport architecture** — the same server simultaneously handles WebTransport (QUIC/HTTP3) clients from browsers and legacy TCP+UDP native clients, with both transports sharing the same player state and world broadcast. The transport layer is abstracted so game logic doesn't care which path a player is using.
- **Custom binary protocol** — all nine message types are hand-serialized in big-endian binary with no external serialization library. The `protocol/` crate is shared between the server and is the single source of truth for wire format.
- **Browser-native multiplayer** — the Godot client runs in the browser via HTML5 export and uses `JavaScriptBridge` to call a hand-written WebTransport JavaScript bridge (`webtransportBridge`), enabling browser clients to participate in the same game session as desktop clients.
- **Authoritative hit detection** — the server owns all collision and damage logic. Melee hits are validated server-side by computing 3D Euclidean distance between players; clients cannot self-report hits.
- **Position reconciliation with drift logging** — the client tracks the delta between its locally-predicted position and the server-authoritative position each frame, logs it as a time-series, and exports it as CSV (F2 key). An in-game drift overlay (F3) shows live per-player drift for latency analysis.
- **Cloud deployment** — server runs on AWS EC2 with a domain name and Let's Encrypt TLS certificates, required for WebTransport (QUIC requires TLS).

---

## Architecture

### Components

```
fps_game/          Godot 4 client (GDScript)
server/            Authoritative Rust game server (Tokio async)
protocol/          Shared Rust binary protocol library
```

### Transport Layer

The server supports two transports simultaneously:

| Transport | Clients | Reliable path | Unreliable path |
|---|---|---|---|
| WebTransport (QUIC) | Browser (HTML5) | Unidirectional QUIC streams | QUIC datagrams |
| Legacy TCP + UDP | Native desktop | TCP port 7777 | UDP port 7778 |

WebTransport is the primary path. The browser client uses a JavaScript bridge (`webtransportBridge`) called via Godot's `JavaScriptBridge.eval()`. Native clients use raw TCP/UDP sockets.

### Protocol (`protocol/src/lib.rs`)

Custom hand-written binary protocol, big-endian, no external serialization dependencies.

| Message | Type byte | Direction | Transport |
|---|---|---|---|
| `Connect` | `0x01` | client → server | reliable |
| `Connected` | `0x10` | server → client | reliable |
| `PlayerInput` | `0x02` | client → server | unreliable |
| `WorldState` | `0x11` | server → client | unreliable |
| `Swing` | `0x03` | client → server | reliable |
| `SwingNotify` | `0x04` | server → client | unreliable |
| `HealthUpdate` | `0x13` | server → client | reliable |
| `YouDied` | `0x14` | server → client | reliable |
| `RespawnRequest` | `0x15` | client → server | reliable |

Each message type implements `serialize() -> Vec<u8>` and `deserialize(buf) -> Option<Self>`. Round-trip tests live in `protocol/src/lib.rs`.

**PlayerInput wire layout (20 bytes):**
```
[0]      message type (0x02)
[1-2]    player_id (u16 be)
[3-4]    seq_num (u16 be)
[5-8]    yaw (f32 be, radians)
[9-12]   pitch (f32 be, radians)
[13]     move_x (i8: -1/0/1)
[14]     move_z (i8: -1/0/1)
[15-18]  pos_y (f32 be)
[19]     flags (bit 0 = jump)
```

**WorldState wire layout:**
```
[0]      message type (0x11)
[1-4]    tick (u32 be)
[5]      player_count (u8)
per player (26 bytes):
  [0-1]   player_id (u16 be)
  [2-5]   pos_x (f32 be)
  [6-9]   pos_y (f32 be)
  [10-13] pos_z (f32 be)
  [14-17] yaw (f32 be)
  [18-21] pitch (f32 be)
  [22-25] health (i32 be)
```

### Server (`server/src/`)

Tokio async runtime, single process.

| Module | Responsibility |
|---|---|
| `main.rs` | Startup, WebTransport endpoint, accepts incoming WT connections |
| `wt.rs` | Per-client WebTransport handler (datagrams + streams) |
| `legacy.rs` | Legacy TCP listener + UDP input processor for native clients |
| `game.rs` | World snapshot, broadcast loop (~60Hz WT / ~20Hz UDP), swing/hit detection, respawn |
| `player.rs` | `Player` struct, movement physics, shared `Players` type |
| `state.rs` | Global atomic counters (player ID, world tick) |
| `cert.rs` | TLS certificate loading for WebTransport endpoint |

Game logic runs server-side:
- **Movement**: server applies `PlayerInput` using the same speed/direction math as the Godot physics controller so positions stay in sync.
- **Hit detection**: a swing checks 3D distance to all alive players; anyone within 2 units takes 25 damage.
- **World bounds**: player X and Z are clamped to ±19 units server-side.

### Client (`fps_game/scripts/`)

| Script | Responsibility |
|---|---|
| `NetworkManager.gd` | Owns all sockets/WebTransport; sends `PlayerInput` each frame; routes incoming messages |
| `LocalPlayer.gd` | FPS controller (WASD + mouse look), swing input, health HUD, death/respawn screen |
| `RemotePlayer.gd` | Represents other players; receives interpolated position/rotation from `WorldState` |
| `Main.gd` | Spawns/despawns `RemotePlayer` nodes as players join or leave |
| `main_menu.gd` | Server selection UI (cloud / local / Tokyo) |

The client detects at runtime whether it is running as a web export (`OS.get_name() == "Web"`) and switches between WebTransport and legacy TCP+UDP accordingly.

### Network Flow

```
Client                                    Server
  |                                          |
  |-- Connect (reliable) ------------------->|  TCP handshake / WT stream
  |<-- Connected + player_id (reliable) -----|
  |                                          |
  |-- PlayerInput (unreliable, every frame)->|  movement applied to player state
  |<-- WorldState (unreliable, ~60Hz) -------|  all player positions/health
  |                                          |
  |-- Swing (reliable) --------------------->|  server checks hit radius, deals damage
  |<-- HealthUpdate (reliable) --------------|  victim receives new HP
  |<-- YouDied (reliable) ------------------|  if HP <= 0
  |<-- SwingNotify (unreliable, broadcast) --|  all clients play swing animation
  |                                          |
  |-- RespawnRequest (reliable) ------------>|  reset HP + position server-side
```

---

## What Was Challenging

- **WebTransport in the browser** — Godot's HTML5 export has no native WebTransport API. All WebTransport calls go through `JavaScriptBridge.eval()` into a hand-written JS bridge. Passing binary data between GDScript and JavaScript required serializing `PackedByteArray` as comma-separated integer strings and parsing them back, since `JavaScriptBridge` cannot pass raw typed arrays.
- **TLS requirements for WebTransport** — QUIC requires TLS everywhere. Local development required generating self-signed certificates and pinning their SHA-256 fingerprint in the client; cloud deployment required a real Let's Encrypt cert and a domain name. Managing which cert mode is active for each environment added deployment complexity.
- **Shared mutable state across async tasks** — the `Players` map is an `Arc<Mutex<HashMap>>` shared between the WebTransport handler, legacy TCP/UDP handler, and world broadcast loop. Avoiding deadlocks required carefully scoping mutex guards, especially around `async` await points where holding a lock would block the executor.
- **Position reconciliation without prediction** — the client sends its own position as part of `PlayerInput` (pos_y for vertical) but does not do full client-side prediction. Reconciling the server's authoritative position with the client's rendered position required tuning the lerp factor to avoid visible snapping while still correcting real drift.

---

## What We Learned

- How to design a compact binary protocol from scratch: field ordering, endianness, type bytes, and length encoding all have real tradeoffs in parsing complexity and bandwidth.
- The difference between reliable and unreliable message delivery in practice — why world state and player input tolerate loss while damage and death events cannot.
- WebTransport as a protocol: it runs over HTTP/3 (QUIC), requires TLS, and exposes both datagrams (unreliable) and unidirectional/bidirectional streams (reliable), making it a natural fit for replacing the split TCP+UDP transport.
- Authoritative server design: putting game logic (hit detection, bounds checking, health) on the server prevents client cheating and keeps all clients consistent, but it requires the server to faithfully reproduce client-side physics.
- AWS EC2 deployment: instance lifecycle, security groups, domain routing, and obtaining TLS certificates with Let's Encrypt for a production WebTransport endpoint.

---

## AI Tools

Claude Code (Anthropic) was used throughout development as a code assistant:
- Drafting initial implementations of the binary serialization/deserialization functions and the Tokio async server structure.
- Debugging the WebTransport handshake flow and diagnosing why the browser client was not receiving the `Connected` response.
- Explaining Godot's `JavaScriptBridge` API and suggesting the CSV-string approach for passing binary data across the GDScript/JS boundary.
- Writing the `apply_movement` function to match Godot's Y-rotation coordinate system.

**Benefits**: Claude significantly accelerated implementation of boilerplate-heavy code (per-message serialize/deserialize pairs, byte-packing helpers) and was useful for reasoning through async lifetime and locking issues in Rust.

**Drawbacks**: Suggestions occasionally needed correction for Godot-specific APIs (GDScript 4 vs 3 differences) and for project-specific invariants (e.g., the exact wire layout we had already committed to). Generated code always required review before use.

---

## What We Would Do Differently

- **Full client-side prediction** — the current client sends input and waits for the server to echo position back. A proper prediction system would apply movement locally and reconcile against the server's authoritative state, eliminating the 1-RTT of visible lag.
- **Delta-compressed world state** — broadcasting every player's full state every frame is wasteful. Sending only changed fields would reduce bandwidth proportionally to the number of players.
- **Separate the protocol crate properly** — `protocol/` is currently a path dependency only; publishing it or structuring the workspace so client and server cannot diverge on wire format would make protocol changes safer.
- **Environment-based configuration** — server IP, port, and cert fingerprint are currently compile-time constants in `NetworkManager.gd`. A config file or exported variable would make switching between local/cloud/Tokyo environments less error-prone.

---

## Build & Run

### Server

```bash
cd server && cargo build --release
cd server && cargo run
```

Ports: TCP/UDP 7777 (WebTransport endpoint), UDP 7778 (legacy UDP for native clients).

### Protocol tests

```bash
cd protocol && cargo test
```

### Client

Open `fps_game/` in Godot Editor 4.2+ and run from the editor, or export to HTML5 and serve with:

```bash
python3 fps_game/serve.py          # HTTP (localhost only)
python3 fps_game/serve.py 8000 cert.pem key.pem   # HTTPS (required for WebTransport)
```

### Infrastructure (AWS EC2)

```bash
./start-us.sh        # start US instance
./stop-us.sh         # stop US instance
./infra/status.sh    # check instance status
./connect-tokyo.sh   # SSH into Tokyo instance
```

Server IP is configured in `fps_game/scripts/NetworkManager.gd` (`CLOUD_IP`, `LOCAL_IP`, `TOKYO_IP`).
starting new push...
