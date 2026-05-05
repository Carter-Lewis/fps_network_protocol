# FPS Game WebTransport Deployment

This project migrated the game server from native TCP/UDP to QUIC and WebTransport, enabling browser-based play.

## Transport Layers

### Desktop (Godot Editor / Native)
- TCP + UDP (native `StreamPeerTCP` + `PacketPeerUDP`)
- Connects to local server on port 7777
- No changes needed for local development

### Web (HTML5 Export in Browser)
- **WebTransport** over HTTP/3 (QUIC)
- Uses JavaScript bridge: `webtransport_bridge.js`
- For production: routed through Cloudflare tunnel

## Deployment Scenarios

### Local Development
1. Run game server: `cd server && cargo run --bin game-server`
2. Run HTTPS server: `cd fps_game && python3 serve_https.py`
3. Open browser: `https://localhost:8000/FPS%20Networking%20Demo.html`
4. Select "Local" and click "Join"

### Production (AWS Amplify + Cloudflare + EC2)
Follow the detailed guide in: **[AWS_CLOUDFLARE_DEPLOYMENT.md](AWS_CLOUDFLARE_DEPLOYMENT.md)**

Key steps:
1. Deploy game server to EC2 (port 7777)
2. Set up Cloudflare tunnel (proxy EC2 → Cloudflare)
3. Deploy website to AWS Amplify
4. Update client config with your Cloudflare domain
5. Test in browser

## Architecture Diagrams

### Local Testing
```
Browser (localhost:8000)
    ↓ (HTTPS + WebTransport mock)
JavaScript Bridge (webtransport_bridge.js)
    ↓
Godot NetworkManager.gd
    ↓
Game Server (localhost:7777 QUIC)
```

### Production Deployment
```
Browser (HTTPS)
    ↓
Cloudflare (WebTransport tunnel)
    ↓
EC2 Instance (QUIC port 7777)
    ↓
Game Server
```

Website hosted on AWS Amplify (separate static assets).

## Files of Interest

- **Server**: `server/src/main.rs` — QUIC endpoint, player management, world state broadcast
- **Client**: `fps_game/scripts/NetworkManager.gd` — transport abstraction (TCP/UDP for desktop, WebTransport for web)
- **Bridge**: `fps_game/webtransport_bridge.js` — WebTransport JavaScript API wrapper
- **Export Config**: `fps_game/export_presets.cfg` — Godot HTML5 export settings (includes bridge JS)

## Configuration

### Server IPs (NetworkManager.gd)
```gdscript
const CLOUD_IP := "3.218.9.34"  # AWS: replace with your Cloudflare domain
const LOCAL_IP := "127.0.0.1"
const TOKYO_IP := "57.181.105.56"
```

For production, change `CLOUD_IP` to your Cloudflare domain (e.g., `game.example.com`).

## Building & Running

### Server
```bash
cd server
cargo build --release
cargo run --bin game-server
```

### Client (Web Export)
```bash
# Export from Godot editor to fps_game/
# Then serve:
cd fps_game
python3 serve_https.py
# Open: https://localhost:8000/FPS%20Networking%20Demo.html
```

## Protocol

Message format (binary):
- `MSG_CONNECT (0x01)`: Client requests connection
- `MSG_CONNECTED (0x10)`: Server assigns player_id
- `MSG_PLAYER_INPUT (0x02)`: Client sends input (position, rotation, etc.)
- `MSG_WORLD_STATE (0x11)`: Server broadcasts player states

See `protocol/src/lib.rs` for serialization details.

## Deployment Checklist

- [ ] EC2 instance running with game server
- [ ] Cloudflare tunnel established
- [ ] Website deployed to AWS Amplify
- [ ] Client config updated with Cloudflare domain
- [ ] HTML5 export re-built with new config
- [ ] Browser test: website loads and connects to game server
- [ ] Game works: players appear, can move and see others

See [AWS_CLOUDFLARE_DEPLOYMENT.md](AWS_CLOUDFLARE_DEPLOYMENT.md) for detailed steps.
