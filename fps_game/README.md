# FPS Networking Demo - Godot Client

## Project Structure

```
fps_game/
├── project.godot
├── scenes/
│   ├── Main.tscn          - Game world, arena, spawns all players
│   └── RemotePlayer.tscn  - One instance per remote player
└── scripts/
    ├── NetworkManager.gd  - THE INTEGRATION POINT (replace with Rust protocol)
    ├── LocalPlayer.gd     - FPS controller for the local player
    ├── RemotePlayer.gd    - Driven by network state, no input
    └── Main.gd            - Spawns/despawns players, shows drift UI
```

## Architecture

```
Godot (rendering + input only)
    LocalPlayer.gd       → reads WASD/mouse, sends to NetworkManager
    RemotePlayer.gd      → receives state from NetworkManager, interpolates
    Main.gd              → spawns remote players on join signal

NetworkManager.gd        ← THIS IS WHERE YOUR RUST PROTOCOL PLUGS IN
    send_player_input()  → encode into your binary format, send over UDP
    send_shoot_event()   → encode into your binary format, send RELIABLY
    receive_world_state()← called by your UDP listener when a packet arrives
    receive_hit_confirmed← called by your UDP listener for reliable messages

Rust Server (GCP/AWS)
    Authoritative state
    Reconciliation
    Lag compensation
    Network simulation per player
    Drift metrics logging
```

## Running Locally (No Server Yet)

Open in Godot 4.2+. The NetworkManager spawns 5 dummy players automatically
for testing. You can move around and the remote players sit stationary.

Set `simulate_local = false` in NetworkManager when connecting to Rust server.

## Connecting Your Rust Protocol

In NetworkManager.gd:

1. Replace `_stub_loopback()` with actual UDP socket send
2. Start a UDP listener thread that calls `receive_world_state()` when packets arrive
3. Set `simulate_local = false`

You can use Godot's built-in `PacketPeerUDP` for the socket, or call into
a GDExtension Rust library for the full protocol stack.

## Drift Evaluation

- Drift is logged automatically in `RemotePlayer.gd` when state updates arrive
- Displayed live on screen (top-left corner)
- Press F2 to save drift_log.csv to the Godot user data directory
- Import the CSV into Python/matplotlib to generate graphs for your presentation

## Controls

- WASD - move
- Mouse - look
- Left click - shoot
- Space - jump
- Escape - release mouse
- F1 - recapture mouse
- F2 - save drift log to CSV
