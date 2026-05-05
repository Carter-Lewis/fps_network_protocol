[![Review Assignment Due Date](https://classroom.github.com/assets/deadline-readme-button-22041afd0340ce965d47ae6ef1cefeee28c7c493a6346c4f15d667ab976d596c.svg)](https://classroom.github.com/a/mun0VlAh)
Goal: Apply the knowledge you've learned in new ways.

# FPS Networking Game - WebTransport Migration

## Project Overview

This project migrates a multiplayer FPS game from traditional TCP/UDP sockets to **QUIC with WebTransport**, enabling real-time networked gameplay in web browsers.

### Key Achievements
- ✅ **Server**: Rewrote networking stack in Rust using `quinn` QUIC library
- ✅ **Protocol**: Maintained binary protocol for efficient message serialization
- ✅ **Client Bridge**: Created JavaScript WebTransport wrapper for browser compatibility
- ✅ **Deployment**: Architecture designed for AWS (Amplify + Cloudflare + EC2)

### Technologies
- **Server**: Rust (quinn 0.10, tokio, QUIC/UDP)
- **Client**: GDScript (Godot 4.6) with JavaScript bridge
- **Transport**: WebTransport (HTTP/3 + QUIC datagrams)
- **Deployment**: AWS Amplify (static assets), Cloudflare (WebTransport tunnel), EC2 (game server)

## Getting Started

### Local Development
```bash
# Terminal 1: Start game server
cd server
cargo run --bin game-server

# Terminal 2: Serve HTML5 export over HTTPS
cd fps_game
python3 serve_https.py

# Terminal 3: Open browser
# https://localhost:8000/FPS%20Networking%20Demo.html
# Select "Local" server and click "Join"
```

### Production Deployment
See [AWS_CLOUDFLARE_DEPLOYMENT.md](AWS_CLOUDFLARE_DEPLOYMENT.md) for:
- EC2 instance setup (game server)
- Cloudflare tunnel configuration (WebTransport proxy)
- AWS Amplify deployment (static website)
- Testing and troubleshooting

Quick reference: [DEPLOYMENT_GUIDE.md](DEPLOYMENT_GUIDE.md)

## Architecture

### Network Stack
- **Desktop**: Native TCP/UDP → Godot's `StreamPeerTCP` / `PacketPeerUDP`
- **Web**: WebTransport (HTTP/3) → JavaScript bridge → Godot networking
- **Server**: QUIC endpoint with datagram support

### Message Flow
```
Browser Client
    ↓ (WebTransport send_datagram)
JavaScript Bridge (webtransport_bridge.js)
    ↓ (JavaScriptBridge.eval)
Godot NetworkManager (GDScript)
    ↓ (packet serialization)
Game Server (Rust, QUIC)
    ↓ (world state broadcast)
All Connected Players
```

## Repository Structure

```
├── server/
│   ├── src/main.rs                    # QUIC server, player mgmt, world broadcast
│   ├── src/bin/quic_smoke.rs          # QUIC smoke test client
│   ├── src/bin/legacy_smoke.rs        # TCP/UDP smoke test client
│   ├── src/bin/webtransport_gateway.rs # (Prototype) WebTransport proxy
│   └── Cargo.toml                     # Dependencies: quinn, tokio, rustls, rcgen
├── protocol/
│   └── src/lib.rs                     # Message types & serialization
├── fps_game/
│   ├── scripts/NetworkManager.gd      # Transport abstraction (web/desktop)
│   ├── webtransport_bridge.js         # JS WebTransport API wrapper
│   ├── scenes/
│   ├── assets/
│   ├── project.godot
│   ├── export_presets.cfg             # HTML5 export with bridge JS
│   └── FPS Networking Demo.html       # Exported HTML5 build
├── AWS_CLOUDFLARE_DEPLOYMENT.md       # Detailed production setup guide
├── DEPLOYMENT_GUIDE.md                # Quick reference & architecture
└── README.md                          # This file
```

## Configuration

Edit `fps_game/scripts/NetworkManager.gd` to change server IPs:

```gdscript
const CLOUD_IP := "3.218.9.34"      # AWS: replace with your Cloudflare domain
const LOCAL_IP := "127.0.0.1"
const TOKYO_IP := "57.181.105.56"
```

For production, set `CLOUD_IP` to your Cloudflare domain (e.g., `game.example.com`).

## Protocol Details

Binary messages over QUIC datagrams or reliable streams:

| Message | ID | Purpose |
|---------|----|---------| 
| MSG_CONNECT | 0x01 | Client requests join |
| MSG_CONNECTED | 0x10 | Server assigns player_id |
| MSG_PLAYER_INPUT | 0x02 | Client: pos, rotation, input state |
| MSG_WORLD_STATE | 0x11 | Server: broadcast all players |

See `protocol/src/lib.rs` for serialization format.

## Testing

### Unit Tests
```bash
cd server
cargo test
```

### Smoke Tests
```bash
# QUIC datagram client
cargo run --bin quic_smoke

# Legacy TCP/UDP client
cargo run --bin legacy_smoke
```

### Integration (Local)
1. Start server: `cargo run --bin game-server`
2. Start HTTPS server: `python3 fps_game/serve_https.py`
3. Open: `https://localhost:8000/FPS%20Networking%20Demo.html`
4. Select server and click "Join"
5. Check browser console for WebTransport logs

## Known Limitations & Future Work

- **Local testing**: Cannot test full WebTransport flow locally (requires HTTP/3 upgrade). Use AWS deployment for end-to-end testing.
- **Authentication**: Not implemented; add token-based auth for production.
- **Persistent state**: Currently in-memory; add RDS/DynamoDB for multi-server deployments.
- **Scalability**: Single EC2 instance; add auto-scaling groups and load balancing for high load.

## Cloud Deployment Checklist

- [ ] AWS account created
- [ ] Cloudflare domain configured
- [ ] EC2 instance launched with game server
- [ ] Cloudflare tunnel established
- [ ] Website deployed to Amplify
- [ ] Client config updated with Cloudflare domain
- [ ] HTML5 re-exported and deployed
- [ ] Browser test successful
- [ ] Production monitoring configured

# Project description
This is an open-ended project. Students can extend their BearTV project or do something new from the ground up. Project ideas must be approved by Dr. Freeman.

You must give a **formal presentation** of your project in place of a final exam. Each group will have ~12 minutes to present their work. Each member of the group must speak. You should have slides. Your presentation must include a demo of your project, although it may invlude a pre-recorded screen capture. In your presentation, you should introduce the problem that you addressed, how you addressed it, technical challenges you faced, what you learned, and next steps (if you were to continue developing it).

You may use AI LLM tools to assist with the development of your project, including code assistant tools like GitHub Copilot. If you do use any AI tools, you must describe your use during your presentation.

Unless you get specific approval otherwise, your project **must** include some component deployed on a cloud hosting service. You can use AWS, GCP, Azure, etc. These services have free tiers, and you might consider looking into tiers specifically for students.

**Graudate students enrolled in CSI-5321:** You have additional requirements. See the bottom of the README.

## Milestones
- You must present your project idea to Dr. Freeman within the first week to get it approved
- You must meet with Dr. Freeman within the first 3 weeks to give a status update and discuss roadblocks
- See the course schedule spreadhseet for specific dates

## Project Ideas
- Simulate UDP packet loss and packet corruption in BearTV in a non-deterministic way (i.e., don't just drop every Nth packet). Then, extend the application protocol to be able to detect and handle this packet loss.
- Extend the BearTV protocol to support streaming images (or video!) alongside the CC data, and visually display them on the client. This should be done in such a way that it is safely deliver*able* over *any* implementation of IPv4. The images don't have to be relevant to the caption data--you can get them randomly on the server from some image source.
- Do something hands on with a video streaming protocol such as MoQ, DASH, or HLS.
- Implement QUIC
- Develop a new congestion control algorithm and evaluate it compared to existing algorithms in a realistic setting
- Make significant contributions to a relevant open-source repository (e.g., moq-rs)
- Implement a VPN
- Implement a DNS
- Do something with route optimization
- Implement an HTTP protocol and have a simple website demo

--> These are just examples. I hope that you'll come up with a better idea to suit your own interests!

## Libraries

Depending on the project, there may be helpful libraries you find to help you out. However, there may also be libraries that do all the interesting work for you. Depending on the project, you'll need to determine what should be fair game. For example, if your project is to implement HTTP, then you shouldn't leverage an HTTP library that does it for you.

If you're unsure if a library is okay to use, just ask me.

## Languages

The core of your project should, ideally, be written in Rust. Depending on the project idea, however, I'm open to allowing the use of other languages if there's a good reason for it. For me to approve such a request, the use of a different language should enable greater learning opportunities for your group.

# Submission

## Questions
- What is your project?
- What novel work did you do?
- What did you learn?
- What was challenging?
- What AI tools did you use, and what did you use them for? What were their benefits and drawbacks?
- What would you do differently next time?

## What to submit
- Push your working code to the main branch of your team's GitHub Repository before the deadline
- Edit the README to answer the above questions
- On Teams, *each* member of the group must individually upload answers to these questions:
	- What did you (as an individual) contribute to this project?
	- What did the other members of your team contribute?
	- Do you have any concerns about your own performance or that of your team members? Any comments will remain confidential, and Dr. Freeman will try to address them in a way that preserves anonymity.
	- What feedback do you have about this course?

## Grading

Grading will be based on...
- The technical merit of the group's project
- The contribution of each individual group member
- Evidence of consistent work, as revealed during milestone meetings
- The quality of the final presentation

# 5321 Extra requirements

(For graudate students enrolled in CSI-5321)

Your project **must** have a research component to it. That is, you must set out to address an open research question using some networking concepts you've learned in this course. Subject to my approval, this project can be an extension of a project you're currently working on with your advisor.

Along with your code repository and presentation, you must submit a 4+ page paper (plus biblography), in double-column paper using the [ACM LaTeX template](https://www.acm.org/publications/proceedings-template) under the `sigconf` document class. This paper should include a brief introduction, related work, a description of your approach, experimental results, and a conclusion. Your goal should be to have a paper of high enough quality that it can be easily extended for a full submission to a research conference.

All other instructions for the project apply, including the above submission questions.

## 5321 Research ideas

- Investigate the efficacy of congestion control algorithms for QUIC. Design your own improvements for some application, and evaluate it.
- Propose improvements to, or novel applications of, the Media Over QUIC protocol
- Limitations of QUIC IP switching
- Quality adaptation for lossy data streaming of something other than traditional video
- Distributed databases, federation, or blockchain
- Protocol security flaws
- IoT, WiFi 8, or 6G
- CDN performance (e.g., content steering)
- Live video streaming with distributed super-resolution between server/client for higher quality and lower latency
- Mesh networking
- Machine learning for routing protocols

--> These are just examples. I hope that you'll come up with a better idea to suit your own interests!
Need to push to EC2
Need to push to EC2
