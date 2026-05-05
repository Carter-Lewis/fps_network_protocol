# QUIC Server → Browser Migration Plan

## Purpose

The `quicserver` branch was an attempt to migrate the FPS game from raw TCP/UDP to QUIC + WebTransport so the game can run in a browser via AWS Amplify. The migration is **partially implemented** but has several critical gaps that prevent it from working end-to-end. This document:

1. Audits exactly what works and what doesn't in `quicserver`
2. Identifies the fundamental blocker (Quinn ≠ WebTransport)
3. Provides a step-by-step plan to finish the migration so the browser client can play against the cloud server

**Scope:** the goal is one working pipeline:
`Browser → AWS Amplify (HTML5 export) → Cloud server (EC2) over WebTransport → Game state syncs both ways`

---

## Part 1 — State of the `quicserver` Branch

### What was changed

| File | Change |
|---|---|
| `server/Cargo.toml` | Added `tokio`, `quinn 0.10`, `rcgen`, `rustls`, `bytes`, `h3` |
| `server/src/main.rs` | Rewritten as async QUIC server (Quinn 0.10 + tokio) on UDP/7777, with a legacy TCP/UDP path on 7777/7778 still running |
| `server/src/bin/webtransport_gateway.rs` | Standalone gateway that proxies QUIC datagrams to a backend UDP socket (alternative architecture) |
| `server/src/bin/quic_smoke.rs` | Test client that connects via raw QUIC and sends a Connect datagram |
| `server/src/bin/legacy_smoke.rs` | Test client for the legacy TCP/UDP path |
| `fps_game/scripts/NetworkManager.gd` | Added `is_web_build` detection and a `_send_webtransport_packet`/`_read_webtransport_packet` path that calls into JavaScript |
| `fps_game/webtransport_bridge.js` | New JS file that wraps the browser `WebTransport` API (`new WebTransport(url)`, `transport.datagrams.readable.getReader()`, etc.) and exposes sync-style getters for GDScript |
| `fps_game/export_presets.cfg` | New "Web" export preset (threads disabled, COOP/COEP headers via PWA flag) |
| `fps_game/scenes/main_menu.gd` | Now `await`s `connect_to_server()` before scene change |

### What works
- Server compiles with the new Quinn dependencies.
- The legacy TCP/UDP listener still accepts desktop clients on 7777/7778, so the existing Godot desktop client should still work after `git checkout quicserver`.
- The `quic_smoke` binary can connect to the QUIC endpoint as a raw-QUIC client and round-trip a `Connect`/`Connected` datagram.
- GDScript correctly detects the web platform (`OS.get_name() == "Web"`) and routes packets through `JavaScriptBridge.eval`.
- The `webtransport_bridge.js` correctly wraps the browser `WebTransport` API and exposes `connectAsync`, `sendDatagram`, `receiveDatagram`, `isConnectedStatus`, `getConnectionError`.

### What does NOT work — the showstopper

**The Quinn server is a raw QUIC server, not a WebTransport server. Browsers cannot connect to it.**

The browser `WebTransport` API speaks WebTransport-over-HTTP/3, which is a specific protocol layered on top of QUIC:
1. Open an HTTP/3 connection (ALPN `h3`)
2. Send a CONNECT request with `:protocol = webtransport` and `:scheme = https`
3. Wait for `200 OK` — only then does the WebTransport session exist
4. Send/receive datagrams or open uni/bi streams via the `:protocol = webtransport` capsule frame

The current `make_server_config()` in `server/src/main.rs` advertises ALPN `hq-29` (an obsolete experimental HTTP-over-QUIC version). Browsers will not even attempt the TLS handshake because they cannot negotiate `hq-29`. They want `h3`, plus WebTransport-aware HTTP/3 framing on top.

**This means:** the existing `quic_smoke` test can talk to the server because it also speaks raw QUIC, but the browser absolutely cannot. No amount of reverse proxy or DNS work will fix this — the server needs to speak WebTransport.

### Other gaps in the quicserver code

These are smaller than the WebTransport gap above, but each one is required for a real game to work:

1. **`Player` lost its connection handle.** The struct dropped `tcp_stream`/`udp_addr` and never gained a QUIC `Connection` reference, so the server has no way to send messages back to a specific QUIC client (`HealthUpdate`, `YouDied`, `WorldState`, `SwingNotify`, `PlayerLeft`).
2. **No WorldState broadcast on the QUIC path.** The legacy path has the 50ms broadcast loop; the QUIC path has nothing. Browser clients would never receive position updates from other players.
3. **`MSG_SWING` is not handled on the QUIC path.** The datagram match in `handle_quic_client` only covers `MSG_CONNECT` and `MSG_PLAYER_INPUT`. No hit detection, no damage, no swing notify.
4. **`MSG_RESPAWN_REQUEST` not handled on QUIC path.**
5. **`HealthUpdate`, `YouDied`, `PlayerLeft`** never sent on QUIC path.
6. **Variable shadowing bug.** In the `accept_uni` branch, `let player_id = NEXT_PLAYER_ID.fetch_add(...)` shadows the outer `Option<u16>` instead of assigning to it. After the stream returns, the outer `player_id` is still `None`, so subsequent datagram processing for that client fails the `if let Some(pid) = player_id` check.
7. **No disconnect cleanup.** When a QUIC connection closes, its `Player` entry is never removed from the map and other players are never notified.
8. **Self-signed cert issues for browsers.** Even after fixing WebTransport, browsers reject self-signed certs unless one of:
   - The cert is signed by a real CA (Let's Encrypt etc.)
   - Chrome is launched with `--ignore-certificate-errors-spki-list=<sha256>`
   - The server's cert hash is passed as `serverCertificateHashes: [{ algorithm: "sha-256", value: <ArrayBuffer> }]` to `new WebTransport(url, options)` — and this only works for ECDSA (P-256) certs valid for ≤14 days
9. **`webtransport_bridge.js` doesn't pass `serverCertificateHashes`.** Even if the server were a real WebTransport server with a self-signed ECDSA cert, the bridge currently calls `new WebTransport(url)` with no options, so the connection will fail in the browser.
10. **GDScript JS interop bug.** `_read_webtransport_packet()` does `for value in packet` on the result of `JavaScriptBridge.eval(...)`. Iterating a JsObject like that doesn't work — Godot 4 returns a `JavaScriptObject` proxy and indexing/iteration of typed arrays needs `JavaScriptBridge.create_callback` or `length`/`[i]` access. The current code likely crashes or returns empty packets.
11. **The "Cloud" path uses `https://3.218.9.34:7777`.** Browsers refuse to do WebTransport against bare IPs because the cert has to match a hostname. Need a real DNS name.
12. **No COOP/COEP headers configured for Amplify.** The export preset enables `progressive_web_app/ensure_cross_origin_isolation_headers=true`, but Amplify needs to actually serve those headers on top of the static files.
13. **The `webtransport_gateway.rs` binary has the same `hq-29` ALPN problem** — it's not a WebTransport server either, just another raw-QUIC endpoint.
14. **`Cargo.toml` is missing the `h3-webtransport` and `wtransport` crates** that would actually implement WebTransport.

---

## Part 2 — The Fundamental Decision: Which WebTransport Stack?

The Rust ecosystem has three plausible options:

| Option | Crate | Pros | Cons |
|---|---|---|---|
| **A** | `wtransport` (BiagioFesta/wtransport) | Cleanest API, async, batteries-included server + client, generates self-signed ECDSA certs with the right lifetime, exposes the cert hash for browser pinning. Used by other game servers. | One more dep |
| **B** | `web-transport-quinn` (Cloudflare) | Lower-level, closer to Quinn | More boilerplate; requires building HTTP/3 + WebTransport framing manually with `h3` + `h3-webtransport` |
| **C** | Quinn + custom HTTP/3 + WebTransport implementation | Full control | A lot of code to write; almost certainly will not work without weeks of debugging |

**Recommendation: use `wtransport`.** It's the closest API match to Quinn (so the existing code structure transfers cleanly), it handles the certificate lifecycle Chrome demands, and it has good documentation. The rest of this guide assumes `wtransport`.

Crates needed in `server/Cargo.toml`:
```toml
wtransport = "0.6"      # WebTransport server — wraps quinn under the hood
tokio      = { version = "1", features = ["full"] }
rcgen      = "0.13"     # generates ECDSA self-signed cert with the right key type
sha2       = "0.10"     # to compute the cert hash for browser pinning
base64     = "0.22"     # to print the hash in a copy-pasteable form
```

The `quinn`, `rustls`, `h3`, `bytes` deps from quicserver can be removed (they're transitive of `wtransport`).

---

## Part 3 — Step-by-Step Migration Plan

This is the order to execute. Each phase has a verification step before moving on.

### Phase 0 — Branch hygiene

```bash
# Start from quicserver as the base, but make a fresh working branch
git checkout quicserver
git checkout -b browser-migration

# Pull the missing pieces from main if needed (Cargo.lock, anything else)
```

### Phase 1 — Replace Quinn with wtransport in the server

**Goal:** server speaks real WebTransport. Browsers can complete the TLS+HTTP/3+WT handshake. (No game logic yet — this phase just gets a "client connected" log line.)

#### 1.1 Update `server/Cargo.toml`

Replace the `[dependencies]` section with:

```toml
[dependencies]
protocol = { path = "../protocol" }
rand     = "0.10.1"

tokio  = { version = "1", features = ["full"] }
wtransport = "0.6"
rcgen  = "0.13"
sha2   = "0.10"
base64 = "0.22"
```

Remove `quinn`, `rustls`, `bytes`, `h3` if they were left over.

The `webtransport_gateway.rs` and `quic_smoke.rs` binaries can be deleted — they were experiments that don't fit the wtransport model. Keep `legacy_smoke.rs` if you still want a TCP/UDP smoke test for desktop clients.

#### 1.2 Rewrite the certificate setup

Create a helper that generates an ECDSA P-256 self-signed cert valid for 14 days (Chrome's max for `serverCertificateHashes`) and prints its SHA-256 hash so you can paste it into the JS bridge.

```rust
// in server/src/main.rs
use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256};
use sha2::{Digest, Sha256};

fn make_or_load_cert() -> (Vec<u8>, Vec<u8>, String) {
    // (cert_der, key_der, sha256_b64)
    let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)
        .expect("ECDSA keygen failed");
    let mut params = CertificateParams::new(vec!["localhost".into()])
        .expect("cert params");

    // Chrome enforces ≤ 14 days validity for cert-hash WebTransport.
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after  = now + time::Duration::days(13);

    let cert = params.self_signed(&key_pair).expect("self-signed");
    let cert_der = cert.der().to_vec();
    let key_der  = key_pair.serialize_der();

    let mut hasher = Sha256::new();
    hasher.update(&cert_der);
    let hash = hasher.finalize();
    let b64 = base64::engine::general_purpose::STANDARD.encode(hash);

    println!("[CERT] SHA-256 fingerprint (base64): {}", b64);
    println!("[CERT] Paste this into webtransport_bridge.js as the certificate hash");
    (cert_der, key_der, b64)
}
```

#### 1.3 Replace `make_server_config` with a wtransport `ServerConfig`

```rust
use wtransport::{Endpoint, Identity, ServerConfig, tls::Certificate, tls::PrivateKey};

async fn build_endpoint() -> Endpoint<wtransport::endpoint::endpoint_side::Server> {
    let (cert_der, key_der, _hash_b64) = make_or_load_cert();
    let identity = Identity::new(
        wtransport::tls::CertificateChain::single(Certificate::from_der(cert_der).unwrap()),
        PrivateKey::from_der_pkcs8(key_der),
    );

    let config = ServerConfig::builder()
        .with_bind_default(7777)        // UDP/7777
        .with_identity(identity)
        .keep_alive_interval(Some(Duration::from_secs(3)))
        .build();

    Endpoint::server(config).expect("endpoint")
}
```

#### 1.4 Replace the connection accept loop

In `main()`:

```rust
#[tokio::main]
async fn main() {
    let players: Players = Arc::new(Mutex::new(HashMap::new()));

    // Keep legacy TCP/UDP listener for desktop backward compat (unchanged).
    tokio::spawn(run_legacy_tcp_udp(players.clone(), Default::default()));

    let endpoint = build_endpoint().await;
    println!("[*] WebTransport server listening on UDP 7777");

    loop {
        let incoming = endpoint.accept().await;
        let session_request = match incoming.await {
            Ok(req) => req,
            Err(e) => { println!("[!] handshake failed: {e}"); continue; }
        };
        println!("[+] WT request: path={} authority={}",
            session_request.path(), session_request.authority());
        let connection = match session_request.accept().await {
            Ok(c) => c,
            Err(e) => { println!("[!] WT accept failed: {e}"); continue; }
        };
        let players = players.clone();
        tokio::spawn(handle_wt_client(connection, players));
    }
}
```

#### 1.5 Stub `handle_wt_client`

Just enough to confirm the handshake works:

```rust
async fn handle_wt_client(conn: wtransport::Connection, _players: Players) {
    println!("[+] WT client connected: {}", conn.remote_address());
    loop {
        tokio::select! {
            dgram = conn.receive_datagram() => match dgram {
                Ok(d) => println!("[>] datagram: {} bytes", d.payload().len()),
                Err(e) => { println!("[-] dgram closed: {e}"); break; }
            },
            stream = conn.accept_uni() => match stream {
                Ok(_s) => println!("[>] uni stream"),
                Err(e) => { println!("[-] stream accept closed: {e}"); break; }
            }
        }
    }
}
```

#### 1.6 Verify (Phase 1)

1. `cd server && cargo run` — should print the cert hash and "WebTransport server listening on UDP 7777".
2. Open Chrome to `chrome://flags`, ensure **WebTransport** is enabled (it is by default in Chrome 118+).
3. In a browser DevTools console (any HTTPS page), paste:
   ```js
   const hash = base64ToBytes("<paste cert hash here>");
   const wt = new WebTransport("https://localhost:7777/wt", {
       serverCertificateHashes: [{ algorithm: "sha-256", value: hash }]
   });
   await wt.ready;
   console.log("connected!");
   const w = wt.datagrams.writable.getWriter();
   await w.write(new Uint8Array([1, 0, 0]));
   ```
   (`base64ToBytes` is a 5-line helper.)
4. The Rust server should log `[+] WT client connected` and `[>] datagram: 3 bytes`.

If step 4 prints, Phase 1 is done. If not, the most likely culprits are:
- Cert hash typo (recompute and recopy)
- Browser cached an old failed handshake — restart Chrome
- `--origin-to-force-quic-on=localhost:7777` flag on Chromium might be needed for non-loopback testing

### Phase 2 — Wire the game protocol over wtransport

**Goal:** the existing 9-message protocol round-trips correctly over WebTransport. After this phase the desktop client still works (legacy path) AND the browser can do the full game loop.

#### 2.1 Restore the connection handle in `Player`

Add back a way to send to a specific WT client. wtransport `Connection` is `Clone`, but cheap to clone. Store it directly:

```rust
struct Player {
    id: u16,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    health: i32,
    alive: bool,
    // legacy paths
    udp_addr: Option<SocketAddr>,
    tcp_stream: Option<Arc<Mutex<std::net::TcpStream>>>,
    // wtransport path
    wt_conn: Option<wtransport::Connection>,
}
```

Add a helper that sends to whichever transport the player is using:

```rust
impl Player {
    async fn send_reliable(&self, data: Vec<u8>) {
        if let Some(c) = &self.wt_conn {
            // open a uni stream for reliable delivery
            if let Ok(mut s) = c.open_uni().await.unwrap_or_else(|e| {
                println!("[!] open_uni failed: {e}"); panic!()
            }).await {
                let _ = s.write_all(&data).await;
                let _ = s.finish().await;
            }
        } else if let Some(tcp) = &self.tcp_stream {
            use std::io::Write;
            let _ = tcp.lock().unwrap().write_all(&data);
        }
    }

    fn send_unreliable(&self, data: Vec<u8>, udp: &Arc<UdpSocket>) {
        if let Some(c) = &self.wt_conn {
            let _ = c.send_datagram(data);
        } else if let Some(addr) = self.udp_addr {
            let _ = udp.try_send_to(&data, addr); // tokio UDP
        }
    }
}
```

> Note: `Connection::send_datagram` is sync in wtransport and returns `Result<(), SendDatagramError>` (it queues into the QUIC datagram buffer, not async).

#### 2.2 Implement full message handling in `handle_wt_client`

This is the largest single change. The structure mirrors the legacy `handle_client` plus the UDP input loop, but everything happens on the same `Connection`.

Pseudo-code outline (write the full thing, but here's the shape):

```rust
async fn handle_wt_client(conn: wtransport::Connection, players: Players, udp: Arc<UdpSocket>) {
    let mut my_id: Option<u16> = None;

    loop {
        tokio::select! {
            // Datagrams: PlayerInput (every frame)
            dgram = conn.receive_datagram() => {
                let Ok(d) = dgram else { break; };
                let bytes = d.payload();
                if bytes.is_empty() { continue; }
                match bytes[0] {
                    MSG_PLAYER_INPUT => apply_player_input(&players, bytes),
                    _ => {} // ignore — Connect comes via stream
                }
            }

            // Uni streams: Connect, Swing, RespawnRequest (reliable)
            stream = conn.accept_uni() => {
                let Ok(mut s) = stream else { break; };
                let mut buf = Vec::with_capacity(64);
                let _ = s.read_to_end(&mut buf, 1024).await;
                if buf.is_empty() { continue; }
                match buf[0] {
                    MSG_CONNECT => {
                        let pid = NEXT_PLAYER_ID.fetch_add(1, Ordering::Relaxed);
                        my_id = Some(pid);
                        players.lock().unwrap().insert(pid, Player {
                            id: pid, pos:[0.0;3], yaw:0.0, pitch:0.0,
                            health: 100, alive: true,
                            udp_addr: None, tcp_stream: None,
                            wt_conn: Some(conn.clone()),
                        });
                        // Send Connected back via a fresh uni stream
                        let resp = Connected { player_id: pid }.serialize();
                        if let Ok(mut out) = conn.open_uni().await.unwrap().await {
                            let _ = out.write_all(&resp).await;
                            let _ = out.finish().await;
                        }
                        println!("[+] WT player {pid} connected");
                    }
                    MSG_SWING => handle_swing(&players, &conn, &udp, &buf).await,
                    MSG_RESPAWN_REQUEST => handle_respawn(&players, &buf),
                    _ => {}
                }
            }
        }
    }

    // Disconnect cleanup
    if let Some(pid) = my_id {
        players.lock().unwrap().remove(&pid);
        broadcast_player_left(&players, &udp, pid).await;
        println!("[-] WT player {pid} disconnected");
    }
}
```

`apply_player_input`, `handle_swing`, `handle_respawn`, `broadcast_player_left` should be free functions that the legacy path can also call — extract them out of the existing legacy `handle_client` so both paths share game logic.

The swing handler must send `HealthUpdate` and `YouDied` to the *target* player (which may be on a different transport — use `Player::send_reliable`), and `SwingNotify` to all other players (use `Player::send_unreliable`).

#### 2.3 Add the WorldState broadcast for WT clients

The legacy 50ms broadcast loop currently iterates `udp_clients`. Add a parallel iteration over WT clients. Cleanest is to change the broadcast to iterate the players map and dispatch on the transport:

```rust
async fn broadcast_loop(players: Players, udp: Arc<UdpSocket>) {
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    loop {
        tick.tick().await;
        let snapshot = snapshot_world(&players);
        let players_g = players.lock().unwrap();
        for p in players_g.values() {
            if let Some(c) = &p.wt_conn {
                let _ = c.send_datagram(snapshot.clone());
            } else if let Some(addr) = p.udp_addr {
                let _ = udp.try_send_to(&snapshot, addr);
            }
        }
    }
}
```

Make sure to spawn this once in `main()`. The legacy broadcast loop in `run_legacy_tcp_udp` should be removed (unify into one loop) so we don't double-send.

#### 2.4 Verify (Phase 2)

1. `cd server && cargo run` — server should boot, print the cert hash, and announce both legacy and WT listeners.
2. Run the *desktop* Godot client (`fps_game/` opened in editor, "Local" server) — confirm it connects, can move, see other players. This proves the legacy path still works.
3. Open two desktop instances — confirm they see each other and can swing/hit.
4. Don't worry about the browser yet — Phase 3 fixes the JS bridge.

### Phase 3 — Fix the browser client

**Goal:** the HTML5 export from Godot can connect, send input, receive WorldState, and render other players.

#### 3.1 Fix the JS bridge

Replace `fps_game/webtransport_bridge.js` with a version that:
- Accepts a `serverCertificateHashes` parameter in `connect`
- Uses a single long-lived datagram writer (current code calls `getWriter()` on every send, which is slow and can deadlock)
- Properly returns datagrams to GDScript (the current code returns a `Uint8Array` directly, which Godot proxies — GDScript needs to read it as `length` + indexed access, not iteration)

```js
class WebTransportBridge {
    constructor() {
        this.transport = null;
        this.writer = null;
        this.connected = false;
        this.error = null;
        this.queue = []; // each entry is a Uint8Array
    }

    async connect(url, certHashB64) {
        try {
            const opts = {};
            if (certHashB64) {
                const raw = Uint8Array.from(atob(certHashB64), c => c.charCodeAt(0));
                opts.serverCertificateHashes = [{ algorithm: "sha-256", value: raw }];
            }
            this.transport = new WebTransport(url, opts);
            await this.transport.ready;
            this.writer = this.transport.datagrams.writable.getWriter();
            this.connected = true;
            this._readLoop();
        } catch (e) {
            this.error = String(e);
            this.connected = false;
        }
    }
    connectAsync(url, certHashB64) { this.connect(url, certHashB64); }

    isConnectedStatus() { return this.connected; }
    getConnectionError() { return this.error; }

    sendDatagram(arr /* Array<number> from GDScript */) {
        if (!this.writer) return false;
        this.writer.write(new Uint8Array(arr)).catch(e => console.error(e));
        return true;
    }

    // Returns Array (not Uint8Array) so GDScript iteration works cleanly,
    // or null if queue is empty.
    receiveDatagram() {
        if (this.queue.length === 0) return null;
        const u8 = this.queue.shift();
        const out = new Array(u8.length);
        for (let i = 0; i < u8.length; i++) out[i] = u8[i];
        return out;
    }

    async _readLoop() {
        const reader = this.transport.datagrams.readable.getReader();
        while (this.connected) {
            try {
                const { value, done } = await reader.read();
                if (done) break;
                this.queue.push(value);
            } catch (e) { console.error(e); break; }
        }
    }
}
window.webtransportBridge = new WebTransportBridge();
```

The key change: `receiveDatagram` returns a plain JS Array, not a Uint8Array. This is critical — Godot's `JavaScriptBridge.eval` returns a `JavaScriptObject` proxy, and the only reliable way to extract bytes from it on the GDScript side is plain Array indexing.

#### 3.2 Fix `NetworkManager.gd`

Three problems to fix in the existing file:

**(a)** `_read_webtransport_packet()` does `for value in packet`. Replace with indexed access:

```gdscript
func _read_webtransport_packet() -> PackedByteArray:
    var packet = JavaScriptBridge.eval("webtransportBridge.receiveDatagram();", true)
    if packet == null:
        return PackedByteArray()
    var bytes := PackedByteArray()
    var n: int = int(packet.length)
    bytes.resize(n)
    for i in range(n):
        bytes[i] = int(packet[i])
    return bytes
```

**(b)** The Connect message should go through a uni stream, not a datagram. The server expects reliable delivery for Connect. Add a `_send_webtransport_stream(buf)` method that calls a JS bridge method that opens a fresh uni stream, writes, and closes it. This means extending the bridge:

```js
// in webtransport_bridge.js
async sendStream(arr) {
    if (!this.transport) return false;
    try {
        const w = await this.transport.createUnidirectionalStream();
        const writer = w.getWriter();
        await writer.write(new Uint8Array(arr));
        await writer.close();
        return true;
    } catch (e) { console.error(e); return false; }
}
```

```gdscript
func _send_webtransport_stream(buf: PackedByteArray) -> void:
    JavaScriptBridge.eval("webtransportBridge.sendStream(%s);" % _packed_byte_array_to_js_array(buf), true)
```

Then update `_send_connect_webtransport`, `send_swing`, and `send_respawn_request` to use `_send_webtransport_stream` instead of `_send_webtransport_packet`.

**(c)** Reading reliable responses (Connected, HealthUpdate, YouDied) needs to read from incoming uni streams, not datagrams. Add to the bridge:

```js
// in webtransport_bridge.js — add an incoming stream reader
async _streamLoop() {
    const reader = this.transport.incomingUnidirectionalStreams.getReader();
    while (this.connected) {
        const { value: stream, done } = await reader.read();
        if (done) break;
        const r = stream.getReader();
        const chunks = [];
        while (true) {
            const { value, done } = await r.read();
            if (done) break;
            chunks.push(value);
        }
        const total = chunks.reduce((n, c) => n + c.length, 0);
        const merged = new Uint8Array(total);
        let o = 0; for (const c of chunks) { merged.set(c, o); o += c.length; }
        this.streamQueue.push(merged);
    }
}

receiveStream() {
    if (this.streamQueue.length === 0) return null;
    const u8 = this.streamQueue.shift();
    const out = new Array(u8.length);
    for (let i = 0; i < u8.length; i++) out[i] = u8[i];
    return out;
}
```

(Initialize `this.streamQueue = []` in the constructor and call `this._streamLoop()` after `await this.transport.ready` in `connect`.)

In NetworkManager.gd, add a `_poll_webtransport_streams()` method that drains `receiveStream()` and dispatches Connected/HealthUpdate/YouDied. Call it from `_process` alongside `_poll_udp`.

**(d)** Pass the cert hash to `connectAsync`. Add a constant near the top of NetworkManager.gd:

```gdscript
const CERT_HASH_B64 := "<paste from server startup log>"
```

And in `_connect_webtransport`:

```gdscript
JavaScriptBridge.eval("webtransportBridge.connectAsync('%s', '%s');" % [url, CERT_HASH_B64], true)
```

For production with a real CA-signed cert, set `CERT_HASH_B64 = ""` and the bridge will skip `serverCertificateHashes`, letting the browser do normal cert validation.

#### 3.3 Fix the WebTransport URL

The current `_webtransport_url()` returns `"https://%s:7777" % server_ip`. Two changes:
- For browser, the URL must include a path. wtransport accepts any path; use `/wt` for clarity.
- The browser cannot use a bare IP. For local: `https://localhost:7777/wt`. For cloud: `https://game.example.com/wt` (you'll need a domain — see Phase 5).

```gdscript
func _webtransport_url() -> String:
    match active_server:
        Server.LOCAL: return "https://localhost:7777/wt"
        Server.CLOUD: return "https://game.yourdomain.com/wt"
        Server.TOKYO: return "https://tokyo.yourdomain.com/wt"
        _: return "https://localhost:7777/wt"
```

#### 3.4 Verify (Phase 3)

1. Re-export the Godot project for Web (Project → Export → Web → Export Project).
2. Serve the export over HTTPS locally. The branch already has `fps_game/serve_https.py`. Verify it sets COOP/COEP headers; if not, add:
   ```python
   self.send_header("Cross-Origin-Opener-Policy", "same-origin")
   self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
   ```
3. `python3 fps_game/serve_https.py` and visit `https://localhost:8000/FPS%20Networking%20Demo.html`.
4. Open DevTools Console.
5. Click "Local" → "Join". You should see the WebTransport handshake succeed in the console and the server should log `[+] WT player 1 connected`.
6. Open a second tab AND a desktop Godot client → confirm all three see each other moving.

### Phase 4 — Fix the export preset for browser threading

The current `export_presets.cfg` has `variant/thread_support=false`. This is fine — Godot 4.6 supports HTML5 without threads, and not requiring SharedArrayBuffer dramatically simplifies hosting (no COOP/COEP needed if threads are off).

Decide:
- **Threads off (recommended for this project)** — keep `variant/thread_support=false`, no COOP/COEP needed, Amplify hosts the static files plain. Some performance loss, but the game is small.
- **Threads on** — set `variant/thread_support=true` AND `progressive_web_app/ensure_cross_origin_isolation_headers=true`, AND configure Amplify to serve `Cross-Origin-Opener-Policy: same-origin` / `Cross-Origin-Embedder-Policy: require-corp` on every response.

If you go with threads-on, add a `customHttp.yml` (Amplify-recognized format) at the repo root:

```yaml
customHeaders:
  - pattern: "**/*"
    headers:
      - key: "Cross-Origin-Opener-Policy"
        value: "same-origin"
      - key: "Cross-Origin-Embedder-Policy"
        value: "require-corp"
```

### Phase 5 — Cloud deployment

This is where the quicserver branch's `AWS_CLOUDFLARE_DEPLOYMENT.md` goes wrong. **Cloudflare tunnels do not support WebTransport** as of 2026 — the `service: quic://` ingress type proxies raw QUIC, not WebTransport-over-HTTP/3. Skip Cloudflare. Use one of these instead:

#### Option 1 (simplest) — EC2 with Let's Encrypt cert via DNS-01

1. **Get a domain.** DuckDNS works (free): create `<your-name>.duckdns.org` pointing to the Elastic IP `3.218.9.34`.
2. **Open EC2 security group** for UDP 7777 from `0.0.0.0/0` (and TCP 80 only during cert issuance).
3. **Install certbot** with the DNS-01 plugin for your DNS provider, OR run certbot in standalone mode if you can briefly free port 80. Get a wildcard or `<name>.duckdns.org` cert.
   ```bash
   sudo certbot certonly --standalone -d <name>.duckdns.org
   ```
   Cert lands at `/etc/letsencrypt/live/<name>.duckdns.org/{fullchain.pem,privkey.pem}`.
4. **Modify `make_or_load_cert`** in the server: if `LETSENCRYPT_PATH` env var is set, load cert and key from disk instead of generating self-signed.
5. **Modify NetworkManager.gd** to set `CERT_HASH_B64 := ""` when targeting CLOUD — the browser will validate against Let's Encrypt's CA chain normally.
6. **Update systemd unit** to set `Environment=LETSENCRYPT_PATH=/etc/letsencrypt/live/<name>.duckdns.org` and to grant the binary permission to read the certs (either run as root, or `setcap`, or copy the certs into a user-readable location and reload after each renewal).
7. **Set up cert renewal** with a deploy hook that restarts the game-server unit:
   ```bash
   sudo certbot renew --deploy-hook "systemctl restart game-server"
   ```
8. **Update the GitHub Actions workflow** (`.github/workflows/deploy.yml`) to also rebuild and deploy on quicserver branch pushes.

#### Option 2 — CloudFront + ACM

CloudFront supports HTTP/3 since 2022 and will terminate TLS for you. But CloudFront does NOT support WebTransport datagrams as of 2026; it only proxies HTTP/3 *requests*. So this option does NOT work for our use case unless AWS adds WebTransport datagram support. **Skip it.**

#### Option 3 (cleanest, costs a bit) — `wtransport-managed` on Fly.io

Fly.io supports UDP and HTTP/3 natively. Deploy the Rust server as a Fly app with a Fly-issued cert. Skip the EC2 dance entirely. Out of scope for this plan but worth knowing about.

#### Amplify deploy

The HTML5 export files live in `fps_game/`. Amplify can deploy them as-is.

1. **AWS Console → Amplify → New App → Host web app → connect GitHub** to the `browser-migration` branch.
2. **Build settings:** there's no build, just publish:
   ```yaml
   version: 1
   frontend:
     phases:
       build:
         commands:
           - echo "no build"
     artifacts:
       baseDirectory: fps_game
       files:
         - "FPS Networking Demo.*"
         - "webtransport_bridge.js"
     cache:
       paths: []
   ```
3. **Custom headers** (only if threads-on): set as in Phase 4.
4. **Deploy.** Amplify gives you `https://<branch>.<id>.amplifyapp.com/FPS%20Networking%20Demo.html`.
5. **Test:** open the Amplify URL, select Cloud, click Join. The browser will WebTransport-connect to `https://<your-name>.duckdns.org:7777/wt`, the EC2 server accepts it, and the game runs.

### Phase 6 — Verification end-to-end

1. **Two browsers, one cloud server:** open the Amplify URL in two windows, both join Cloud. Both should appear and be able to hit each other.
2. **One desktop, one browser:** desktop Godot client picks Local server (or rebuild it with Cloud IP and connect to the same EC2). Confirm cross-transport play works. (Both go through the same `Players` map on the server.)
3. **Cert renewal smoke test:** force-renew the Let's Encrypt cert (`certbot renew --force-renewal`) and confirm the deploy-hook restarts the server and the next browser join still works.
4. **Drift logging:** Press F3 in the desktop client, F2 to export — confirm drift CSV has reasonable numbers. (Drift over WebTransport will likely be slightly higher than UDP since datagrams in QUIC have a small but real fragmentation overhead; this is expected and a good thing to measure for the writeup.)

---

## Part 4 — Files You Will Create/Modify (Complete List)

| Path | Action | Notes |
|---|---|---|
| `server/Cargo.toml` | Modify | Replace quinn/h3/rustls/bytes with `wtransport`, `sha2`, `base64` |
| `server/src/main.rs` | Modify | Replace `make_server_config` and accept loop with wtransport equivalents; restore `wt_conn` field on `Player`; implement full message handling for the WT path; unify the WorldState broadcast loop |
| `server/src/bin/quic_smoke.rs` | Delete | No longer relevant — wtransport-side smoke test would use `wtransport::ClientConfig` instead |
| `server/src/bin/webtransport_gateway.rs` | Delete | Was based on a wrong premise — wtransport is the gateway |
| `server/src/bin/legacy_smoke.rs` | Keep | Still useful for testing the legacy path |
| `fps_game/webtransport_bridge.js` | Modify | Pass `serverCertificateHashes`; add stream send/receive; fix `receiveDatagram` to return Array not Uint8Array |
| `fps_game/scripts/NetworkManager.gd` | Modify | Fix `_read_webtransport_packet` iteration; add `_send_webtransport_stream` and `_poll_webtransport_streams`; route Connect/Swing/Respawn through streams; pass cert hash to bridge; fix URL to use hostname |
| `fps_game/serve_https.py` | Verify | Make sure COOP/COEP headers are set if threads enabled |
| `fps_game/export_presets.cfg` | Verify | Decide threads on/off |
| `customHttp.yml` (repo root) | Create (only if threads on) | Amplify custom headers |
| `.github/workflows/deploy.yml` | Modify | Add a step that builds the server on a push to `browser-migration` and SCPs the binary to EC2; restart the systemd unit |
| `infra/setup-letsencrypt.sh` | Create | One-shot script to install certbot, get cert, set up the deploy hook |
| `/etc/systemd/system/game-server.service` (on EC2) | Create | systemd unit pointing at `/home/ubuntu/game-server/server` with `Environment=LETSENCRYPT_PATH=...` |

---

## Part 5 — Risks and Things That Will Bite You

1. **wtransport API churn.** Versions ≤ 0.6 changed signatures. Pin the version exactly (not `wtransport = "0.6"` but `wtransport = "=0.6.x"`) and read the docs for that exact version.
2. **Chrome cert-hash 14-day limit.** If you go the self-signed route, you must regenerate the cert and update the hash in NetworkManager.gd every two weeks. That's why the Let's Encrypt path is recommended for anything beyond local testing.
3. **Datagram size limits.** QUIC datagrams have a path MTU of typically 1200 bytes. The current WorldState format is 2 + 26 × N bytes; with 45 players you'd hit the MTU. Won't matter for a 4-player class demo, but mention it in the writeup.
4. **Browser back/forward navigation kills the WebTransport session.** The bridge should detect `transport.closed` and signal NetworkManager to return to the menu. Currently it doesn't; add it if it bites you.
5. **EC2 UDP egress cost.** UDP datagrams from AWS cost the same as TCP, but a chatty server (50ms broadcast × N players × 1200 bytes) will rack up more than you'd expect. For a class demo this is pennies; for production, monitor.
6. **Godot's HTML5 main thread blocking.** GDScript runs on the main thread. If `_poll_webtransport_streams` does too much work per frame (e.g., parsing many uni streams), frame rate suffers. Cap each frame to N stream reads and continue next frame.
7. **The `JavaScriptBridge.eval` calls leak strings.** Each `eval` call constructs a JS string, parses it, and runs it. For 60 fps × multiple bridge calls per frame, this adds up. If profiling shows it as a hotspot, switch to `JavaScriptBridge.create_callback` + held references to the bridge object. (Premature for the class scope.)

---

## Quick Sanity-Check Commands

```bash
# Phase 1: confirm WT handshake works
cd server && cargo run
# Then in browser DevTools console (any HTTPS page):
# const hash = Uint8Array.from(atob("PASTE"), c => c.charCodeAt(0));
# const wt = new WebTransport("https://localhost:7777/wt", { serverCertificateHashes: [{algorithm:"sha-256", value: hash}] });
# await wt.ready

# Phase 2: confirm desktop legacy path still works
# (open Godot, run, "Local", join)

# Phase 3: confirm browser path works
python3 fps_game/serve_https.py
# Open https://localhost:8000/FPS%20Networking%20Demo.html

# Phase 5: confirm cloud deployment
ssh ubuntu@3.218.9.34 "sudo systemctl status game-server"
# Then open the Amplify URL and join Cloud
```

---

## Summary — What This Plan Buys You

After Phase 5 you have:
- One Rust binary on EC2 that serves both desktop clients (TCP/UDP, port 7777/7778) AND browser clients (WebTransport on UDP/7777 — via the same UDP socket, since QUIC and the legacy UDP coexist on different ALPN/handshake patterns... actually they DON'T share a socket cleanly, **so put the WT server on UDP/7778 and leave the legacy UDP on UDP/7779** — small fix, mention in implementation).
- A Godot HTML5 export hosted on Amplify that connects to the EC2 server over WebTransport.
- A working game loop (move, swing, hit, die, respawn) over both transports.

The single biggest risk is wtransport API churn; allocate one focused afternoon to Phase 1 alone, and the rest follows in a few hours each.
