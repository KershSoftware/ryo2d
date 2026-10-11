# Deploying a Matchbox Signaling Server

A signaling server is required for WebRTC NAT traversal. This guide shows the cheapest way to deploy one.

## Option 1: Fly.io (Recommended - Free Tier)

Fly.io offers a free tier with 3 shared-cpu VMs. Perfect for a signaling server.

### One-Time Setup

```bash
# Install Fly CLI
# Windows (PowerShell)
powershell -Command "iwr https://fly.io/install.ps1 -useb | iex"

# macOS/Linux
curl -L https://fly.io/install.sh | sh

# Create account and login
fly auth signup
```

### Deploy the Server

Create a new directory and add these files:

**Dockerfile**
```dockerfile
FROM rust:1.83-slim as builder
RUN cargo install matchbox_server
FROM debian:bookworm-slim
COPY --from=builder /usr/local/cargo/bin/matchbox_server /usr/local/bin/
EXPOSE 3536
CMD ["matchbox_server"]
```

**fly.toml**
```toml
app = "your-game-matchbox"  # Change this to a unique name
primary_region = "iad"      # Change to nearest: lax, sfo, sea, ord, dfw, atl, mia, ewr, lhr, ams, fra, sin, syd, nrt

[build]

[http_service]
  internal_port = 3536
  force_https = true
  auto_stop_machines = "stop"
  auto_start_machines = true
  min_machines_running = 0

[[vm]]
  memory = "256mb"
  cpu_kind = "shared"
  cpus = 1
```

**Deploy:**
```bash
fly launch --no-deploy    # First time: creates app, say "no" to database/redis
fly deploy                # Deploy (and subsequent updates)
```

Your server will be at: `wss://your-game-matchbox.fly.dev`

### Usage in Ryo2D

```rust
let config = NetworkServerConfig {
    transport: TransportConfig::WebRtc(WebRtcConfig::new(
        "wss://your-game-matchbox.fly.dev",
        "lobby-room",
    )),
    ..Default::default()
};
```

### Costs

- **Free tier**: 3 shared-cpu-1x VMs, 256MB RAM each
- Signaling is lightweight - free tier is plenty for indie games
- If you exceed free tier: ~$2-5/month

---

## Option 2: Railway (Free Tier)

```bash
# Install Railway CLI
npm install -g @railway/cli

# Login and deploy
railway login
railway init
railway up
```

**railway.json**
```json
{
  "$schema": "https://railway.app/railway.schema.json",
  "build": {
    "builder": "DOCKERFILE"
  },
  "deploy": {
    "startCommand": "matchbox_server",
    "restartPolicyType": "ON_FAILURE"
  }
}
```

Use the same Dockerfile as above.

---

## Option 3: Single Command Local Tunnel (Development/Testing)

For quick testing without deploying anything:

```bash
# Terminal 1: Run matchbox server locally
cargo install matchbox_server
matchbox_server

# Terminal 2: Expose via cloudflared tunnel (free)
# Install: https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/install-and-setup/installation/
cloudflared tunnel --url http://localhost:3536
```

This gives you a temporary `https://random-name.trycloudflare.com` URL. Good for playtesting with friends.

---

## Option 4: Dirt Cheap VPS ($3-4/month)

For full control at minimal cost:

### Hetzner Cloud (Europe) - €3.29/month
```bash
# Create CX22 instance (2GB RAM, 20GB disk)
# SSH in, then:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
cargo install matchbox_server

# Run with systemd
sudo tee /etc/systemd/system/matchbox.service << 'EOF'
[Unit]
Description=Matchbox Signaling Server
After=network.target

[Service]
ExecStart=/root/.cargo/bin/matchbox_server
Restart=always
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl enable --now matchbox
```

Then put it behind Caddy or nginx for HTTPS.

### DigitalOcean - $4/month
Same process, use their smallest droplet.

---

## Quick Reference

| Provider | Cost | Setup Time | Best For |
|----------|------|------------|----------|
| Fly.io | Free | 5 min | Production |
| Railway | Free | 5 min | Production |
| Cloudflare Tunnel | Free | 1 min | Testing |
| Hetzner | €3/mo | 15 min | Full control |

## Monitoring

Check if your server is running:
```bash
# Should return WebSocket upgrade response
curl -i -N \
  -H "Connection: Upgrade" \
  -H "Upgrade: websocket" \
  -H "Sec-WebSocket-Key: test" \
  -H "Sec-WebSocket-Version: 13" \
  https://your-game-matchbox.fly.dev/test-room
```
