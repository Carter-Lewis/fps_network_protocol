# Quick Start: Browser WebTransport Testing

## Prerequisites
- Godot 4.6+ installed
- Rust toolchain for building server
- macOS, Linux, or Windows
- Modern browser (Chrome, Edge, or Firefox)

## Step 1: Build the Server

```bash
cd /path/to/finalproject-group9_s26/server
cargo run
```

The server will:
1. Generate self-signed TLS certificate in memory
2. Listen on `https://127.0.0.1:7777`
3. Print debug output to console

Expected output:
```
[GameState] Initializing...
[Quinn] Starting QUIC server on 127.0.0.1:7777
[Server] Accepting connections...
```

## Step 2: Export Game for Web

### Option A: Via Godot GUI
1. Open `fps_game/project.godot` in Godot 4.6
2. Project → Export → HTML5 (Web)
3. Click "Export Project"
4. Choose destination folder (e.g., `./build/web`)
5. Wait for build to complete

### Option B: Via Command Line
```bash
cd /path/to/fps_game
godot --export-web --path . ../build/web
```

Expected output:
- `build/web/index.html` - Main game page
- `build/web/assets/webtransport_bridge.js` - WebTransport bridge
- `build/web/index.wasm` - Game binary
- `build/web/index.js` - Godot engine wrapper

## Step 3: Serve with HTTPS

WebTransport requires HTTPS. Use one of these methods:

### Option 1: Python HTTP Server (Recommended)
```bash
cd /path/to/build/web
python3 -m http.server --cgi 8000 --bind 127.0.0.1
```

Then open: `http://localhost:8000`

Note: You'll see a CORS error in console but the game should still load.

### Option 2: Live Server (VS Code)
1. Install "Live Server" extension in VS Code
2. Open `build/web` folder in VS Code
3. Right-click `index.html` → "Open with Live Server"
4. Browser opens automatically to `http://127.0.0.1:5500`

### Option 3: Node.js HTTP Server
```bash
cd /path/to/build/web
npx http-server -p 8000 --cors
```

## Step 4: Test in Browser

1. Open browser console (F12)
2. Navigate to http://localhost:8000
3. You should see:
   - Main menu with server options
   - Click "Cloud", "Tokyo", or "Local"
   - Game should connect to `https://127.0.0.1:7777`

### Expected Console Output
```
[NetworkManager] Platform: Web, Using WebTransport: true
[WebTransport] Initiating connection...
[WebTransport] URL: https://127.0.0.1:7777
[WebTransport] Waiting for connection... (polling)
[WebTransport] Connected!
[WebTransport] Sent CONNECT message
[WebTransport] Connected! player_id = <some_id>
[Game] Player joined: <some_id>
```

### Troubleshooting

#### Error: "WebTransport is not defined"
- **Cause**: Browser doesn't support WebTransport
- **Solution**: Use Chrome 118+, Edge 118+, or Firefox 121+
- **Check**: Open console and type `new WebTransport()` - should not error

#### Error: "Uncaught DOMException: QuicTransportError"
- **Cause**: Server unreachable or cert rejected
- **Solution**: 
  1. Verify server is running (`cargo run` in server folder)
  2. Check console shows "[Quinn] Accepting connections"
  3. Try `ping 127.0.0.1` in terminal
  4. Check firewall isn't blocking port 7777

#### Error: "SecurityError: Cannot connect"
- **Cause**: Not using HTTPS (WebTransport requirement)
- **Solution**: Access via `https://`, not `http://`
- Note: Self-signed cert is OK for localhost

#### Game loads but can't connect
- **Check 1**: Browser console shows connection status
- **Check 2**: Server console shows incoming connection
- **Check 3**: Verify webtransport_bridge.js is loaded (check Network tab)
- **Check 4**: Try refreshing browser (Cmd+Shift+R for hard refresh)

## Step 5: Verify Gameplay

Once connected:
1. **Check player ID** - Should see a number in the drift label (F3 to toggle)
2. **Move the player** - Use WASD to move, mouse to look
3. **Check drift** - Press F3 to show drift UI in top-right
4. **Test multiple clients** - Open game in another browser tab (same server)
5. **Export drift log** - Press F2 to save `drift_log.csv`

## Tips for Debugging

### Check Network Traffic
1. Open DevTools → Network tab
2. Filter by `webtransport` to see connection details
3. Datagrams won't show individual messages but connection should be active

### Monitor Server
In another terminal, watch server output:
```bash
cd server && cargo run 2>&1 | grep -E "incoming|player|state"
```

### Verify Bridge Loaded
In browser console:
```javascript
console.log(window.webtransportBridge);  // Should print class instance
console.log(webtransportBridge.isConnectedStatus());  // Should print boolean
```

### Check WebTransport Support
```javascript
const isSupported = 'WebTransport' in window;
console.log('WebTransport supported:', isSupported);
```

## Next Steps

After successful local testing:

1. **Deploy to cloud**:
   - Get real SSL certificate (Let's Encrypt recommended)
   - Update `CLOUD_IP` in NetworkManager.gd
   - Run `./start-us.sh` or `./start-tokyo.sh`

2. **Test with remote players**:
   - Share game URL with teammate
   - Both load game in browser
   - Verify you see each other

3. **Performance testing**:
   - Check drift values (should be <1.0 units on good network)
   - Test with 3+ simultaneous players
   - Monitor server CPU/memory usage

4. **Optimize**:
   - Adjust broadcast rate (currently 50ms)
   - Fine-tune network reconciliation threshold (currently 0.5 units)
   - Add compression if bandwidth limited

## Files Reference

| File | Purpose |
|------|---------|
| `server/src/main.rs` | QUIC server listening on port 7777 |
| `fps_game/scripts/NetworkManager.gd` | Transport abstraction layer |
| `fps_game/assets/webtransport_bridge.js` | JavaScript bridge |
| `fps_game/project.godot` | Export configuration |
| `build/web/index.html` | Main game page (generated) |

## Common Commands

```bash
# Build server
cd server && cargo run

# Build game for web
godot --export-web --path ./fps_game ../build/web

# Serve web files (Python)
cd build/web && python3 -m http.server 8000

# Export drift log
# Press F2 in game, then check: ~/.local/share/Godot/app_userdata/FPS\ Networking\ Demo/drift_log.csv
```
