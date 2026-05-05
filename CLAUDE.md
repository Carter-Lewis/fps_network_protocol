# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Multiplayer FPS game built as a networking course project. Three main components communicate via a custom binary protocol:

- **`fps_game/`** — Godot 4.2+ client (GDScript)
- **`server/`** — Rust game server (authoritative state)
- **`protocol/`** — Rust serialization library shared by server

## Build & Run Commands

### Server (Rust)
```bash
cd server && cargo build           # debug build
cd server && cargo build --release # release build
cd server && cargo run             # run server locally
cd protocol && cargo test          # run protocol serialization tests
```

### Client (Godot)
Open `fps_game/` in Godot Editor 4.2+ and run from the editor. The server IP is configured in `NetworkManager.gd`.

### Infrastructure (AWS EC2)
```bash
./start-us.sh       # start US AWS instance
./stop-us.sh        # stop US AWS instance
./infra/status.sh   # check instance status
./connect-tokyo.sh  # connect to Tokyo region instance
```

## Architecture

### Protocol (`protocol/src/lib.rs`)
Custom hand-written binary protocol (big-endian, no external serialization dependencies). Nine message types:

| Message | Direction | Transport |
|---|---|---|
| `Connect` / `Connected` | client→server / server→client | TCP |
| `PlayerInput` | client→server | UDP |
| `WorldState` | server→client | UDP, broadcast every 50ms |
| `Swing` / `SwingNotify` | client→server / server→client | TCP |
| `HealthUpdate` | server→client | TCP |
| `YouDied` / `RespawnRequest` | server→client / client→server | TCP |

Each message type implements `serialize(&self) -> Vec<u8>` and a static `deserialize(buf)` method. Tests in `protocol/` verify round-trip serialization for all types.

### Server (`server/src/main.rs`)
Three threads running concurrently:
1. **TCP listener** — accepts new connections, receives reliable messages (swing, respawn)
2. **UDP input processor** — receives `PlayerInput` messages from all clients
3. **World state broadcaster** — serializes and sends `WorldState` to all clients every 50ms

Ports: TCP 7777, UDP 7778. All game state (position, health, alive status) is authoritative on the server. Hit detection runs server-side.

### Client (`fps_game/scripts/`)
- **`NetworkManager.gd`** — owns TCP/UDP sockets, sends `PlayerInput` each physics tick, receives and dispatches all server messages
- **`LocalPlayer.gd`** — FPS controller: WASD + mouse look, swing/melee input, health state
- **`RemotePlayer.gd`** — represents other players; applies interpolated position/rotation from `WorldState`
- **`Main.gd`** — spawns/despawns `RemotePlayer` instances as players join/leave

### Network Flow
1. Client connects TCP → sends `Connect` → receives `Connected` with assigned player ID
2. Client sends `PlayerInput` (position, rotation, velocity) via UDP each physics frame
3. Server broadcasts `WorldState` (all player positions/health) via UDP every 50ms
4. Melee hits and deaths go over TCP for reliability

## Development Notes

- The devcontainer (`docker.io/acfreeman/rustnetworking`) has NET_ADMIN capability for network simulation/latency testing
- `NetworkManager.gd` has commented-out IP constants for switching between local, US cloud, and Tokyo servers
- `LocalPlayer.gd` logs network drift (client vs. server position delta) for latency analysis
