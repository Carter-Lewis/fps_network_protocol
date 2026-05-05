# AWS + Cloudflare WebTransport Deployment Guide

## Architecture Overview

```
Browser (HTTPS)
    ↓
Cloudflare (WebTransport tunnel)
    ↓
EC2 (Game Server on QUIC port 7777)
    ↓
Amplify (Static HTML5 export)
```

## Phase 1: AWS Setup

### 1.1 Prepare Your Domain

You'll need a domain. For testing, you can use a free domain or AWS Route53 domain.

Example: `game.example.com`

### 1.2 Deploy Game Server on EC2

1. **Launch EC2 Instance**
   - Region: `us-east-1` (or your preferred)
   - OS: Amazon Linux 2 or Ubuntu 22.04
   - Instance type: `t3.medium` (sufficient for testing)
   - Security Groups:
     - Allow inbound UDP 7777 from anywhere (0.0.0.0/0)
     - Allow inbound TCP 22 from your IP (SSH)

2. **SSH into instance and set up Rust**
   ```bash
   sudo yum update -y  # Amazon Linux 2
   # or: sudo apt update && sudo apt upgrade -y  # Ubuntu
   
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source $HOME/.cargo/env
   ```

3. **Clone and build your game server**
   ```bash
   git clone <your-repo> /home/ec2-user/game-server
   cd /home/ec2-user/game-server/server
   cargo build --release --bin game-server
   ```

4. **Create systemd service for game server**
   ```bash
   sudo tee /etc/systemd/system/game-server.service > /dev/null <<EOF
   [Unit]
   Description=Game Server
   After=network.target

   [Service]
   Type=simple
   User=ec2-user
   WorkingDirectory=/home/ec2-user/game-server/server
   ExecStart=/home/ec2-user/game-server/server/target/release/game-server
   Restart=always
   RestartSec=10

   [Install]
   WantedBy=multi-user.target
   EOF
   
   sudo systemctl daemon-reload
   sudo systemctl enable game-server
   sudo systemctl start game-server
   sudo systemctl status game-server
   ```

5. **Verify server is running**
   ```bash
   sudo systemctl logs -f game-server
   # Should see: "[*] QUIC server listening on 0.0.0.0:7777"
   ```

6. **Note your EC2 Public IP**
   - Get from AWS Console → EC2 → Instances
   - Example: `54.123.45.67`

### 1.3 Deploy Website to AWS Amplify

1. **Prepare your repo**
   - Ensure your `fps_game/` folder is in the repo root
   - The exported HTML (`FPS Networking Demo.html`) should be in `fps_game/`

2. **Push to GitHub/GitLab**
   ```bash
   git add .
   git commit -m "Deploy WebTransport setup"
   git push origin main
   ```

3. **Connect Amplify**
   - Go to AWS Amplify Console
   - Click "New App" → "Host Web App"
   - Select your Git provider (GitHub/GitLab) and authorize
   - Choose your repo and branch (`main`)
   - Build settings:
     - Build command: `echo "No build needed"`
     - Publish directory: `fps_game`
   - Deploy

4. **Get Amplify URL**
   - After deployment, Amplify provides a URL: `https://<random>.amplifyapp.com`
   - Or, if you have a custom domain, add it in Amplify settings

---

## Phase 2: Cloudflare WebTransport Tunnel Setup

### 2.1 Cloudflare Account & Domain

1. **Create Cloudflare account** at https://dash.cloudflare.com
2. **Add your domain** (or buy one from Cloudflare)
3. **Update nameservers** to Cloudflare (if using external domain)

### 2.2 Create Cloudflare Tunnel

Cloudflare Tunnels allow you to expose your EC2 game server without opening ports.

1. **Install cloudflared on EC2**
   ```bash
   # On EC2 instance
   sudo wget https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-x86_64
   sudo chmod +x cloudflared-linux-x86_64
   sudo mv cloudflared-linux-x86_64 /usr/local/bin/cloudflared
   ```

2. **Authenticate cloudflared**
   ```bash
   cloudflared tunnel login
   # This will give you a URL to authorize in your browser
   ```

3. **Create tunnel**
   ```bash
   cloudflared tunnel create game-server
   # Note the tunnel ID and credentials file path
   ```

4. **Create tunnel config**
   ```bash
   sudo tee /etc/cloudflared/config.yml > /dev/null <<EOF
   tunnel: game-server
   credentials-file: /root/.cloudflared/<TUNNEL_ID>.json

   ingress:
     - hostname: game.example.com
       service: quic://127.0.0.1:7777
     - service: http_status:404
   EOF
   ```

5. **Create DNS record in Cloudflare**
   - Dashboard → DNS → Add record
   - Type: `CNAME`
   - Name: `game` (so it's `game.example.com`)
   - Target: `<TUNNEL_ID>.cfargotunnel.com`
   - Proxy: Proxied (orange cloud)

6. **Start cloudflared**
   ```bash
   sudo systemctl start cloudflared
   sudo systemctl enable cloudflared
   sudo systemctl status cloudflared
   ```

### 2.3 Enable Cloudflare WebTransport

1. **Dashboard → SSL/TLS → Origin Server**
   - Generate Origin Certificate (or use Let's Encrypt)
   - Note the certificate path

2. **Dashboard → Network → Settings**
   - Enable "HTTP/3 (QUIC)"
   - Enable "0-RTT Connection Resumption"

3. **Dashboard → Rules → Page Rules** (or use Workers for more control)
   - Create rule for `game.example.com`
   - Set "QUIC" to "On"
   - Set "Minimum TLS Version" to TLS 1.2

---

## Phase 3: Update Client Code

### 3.1 Update NetworkManager.gd

Change the `CLOUD_IP` to your Cloudflare domain:

```gdscript
const CLOUD_IP := "game.example.com"  # Change from 3.218.9.34
const LOCAL_IP := "127.0.0.1"
const TOKYO_IP := "57.181.105.56"
```

The `_webtransport_url()` function will now return:
```
https://game.example.com:7777
```

### 3.2 Re-export HTML5

In Godot editor:
1. File → Export → Web
2. Export → Deploy (or just export the files)
3. New HTML5 export will use the updated config

### 3.3 Deploy updated export to Amplify

```bash
# In your project root
rm -rf fps_game/FPS*.html fps_game/FPS*.js fps_game/FPS*.wasm
# Re-export from Godot (see above)
git add fps_game/
git commit -m "Update WebTransport to use Cloudflare domain"
git push origin main
# Amplify will auto-redeploy
```

---

## Phase 4: Testing

### 4.1 Verify Connectivity

**From your local machine:**
```bash
# Test EC2 reachability via Cloudflare
curl -I https://game.example.com
# Should return 200 or connection-related response

# Verify game server on EC2
ssh ec2-user@54.123.45.67 "sudo systemctl status game-server"
```

### 4.2 Test in Browser

1. **Open Amplify URL** in browser:
   - `https://<random>.amplifyapp.com/FPS%20Networking%20Demo.html`
   
2. **Open DevTools** (F12 → Console)

3. **Select "Cloud" server** and click "Join"

4. **Check console for logs:**
   - Should see: `[WebTransport] Connecting to: https://game.example.com:7777`
   - Then: `[WebTransport] Connected successfully` (if all works)

5. **Check EC2 game server logs:**
   ```bash
   ssh ec2-user@54.123.45.67
   sudo systemctl logs -n 20 game-server
   # Should show: "[*] New QUIC client connection from ..."
   ```

### 4.3 Verify Amplify Domain Works

If you added a custom domain to Amplify, test that URL too:
- `https://game.example.com/FPS%20Networking%20Demo.html` (if custom domain)

---

## Phase 5: Production Hardening

Once testing works, do these:

1. **TLS Certificates**
   - Cloudflare auto-manages for your domain
   - EC2 game server uses self-signed (acceptable behind Cloudflare)

2. **Security Groups**
   - Restrict EC2 inbound UDP 7777 to **Cloudflare IPs only**
   - Get list: https://www.cloudflare.com/ips/

3. **Scaling**
   - Use Auto Scaling Group if you expect high load
   - Use RDS/DynamoDB for persistent state (if needed)

4. **Monitoring**
   - CloudWatch alarms for EC2 CPU, memory, network
   - CloudFlare analytics for tunnel health

---

## Troubleshooting

### Browser shows "failed to fetch"
- Check DevTools Network tab for exact error
- Verify `game.example.com` resolves: `nslookup game.example.com`
- Confirm Cloudflare tunnel is running: `sudo systemctl status cloudflared`

### WebTransport connection times out
- SSH to EC2 and check game server: `sudo systemctl status game-server`
- Check firewall: `sudo ufw status` or AWS Security Groups
- Verify QUIC is responding: test with `quic_smoke` client locally

### Cloudflare tunnel not working
- Check credentials: `cat /root/.cloudflared/<TUNNEL_ID>.json`
- Verify DNS record points to `.cfargotunnel.com`
- Check tunnel status: `cloudflared tunnel list` and `cloudflared tunnel run game-server`

---

## Quick Reference

| Component | Location | Port | Notes |
|-----------|----------|------|-------|
| Website | AWS Amplify | 443 (HTTPS) | Static HTML5 export |
| Game Server | EC2 | 7777 (UDP/QUIC) | Rust quinn server |
| Cloudflare Tunnel | EC2 (cloudflared daemon) | - | Proxies QUIC to Cloudflare |
| WebTransport | Browser → Cloudflare → EC2 | 443 + 7777 | Transparent to browser |

---

## Next Steps

1. Launch EC2 and build game server
2. Set up Cloudflare account and tunnel
3. Deploy Amplify
4. Update client code with your domain
5. Test end-to-end
6. Monitor and iterate

Let me know if you hit any blockers!
