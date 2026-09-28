# Smaragd sync server (experimental)

> **Experimental.** Sync is new. The storage format, HTTP API and wire protocol may
> still change between releases in ways that require re-creating vaults, so back up
> your projects independently and don't treat the server as the only copy of anything.

A small, self-hostable server that keeps a Smaragd project in sync across your own
devices. It is a **blind store**: your text is encrypted on your devices before it
is uploaded, and the server only ever holds ciphertext it cannot read. It runs as a
single container (or a single static-ish binary) with one SQLite file for storage.

This is different from Smaragd's *Collaboration* feature, which is live,
peer-to-peer and needs no server at all. Sync is for keeping *your own* devices
identical in the background, even when only one is open at a time.

- [What the server can and can't see](#what-the-server-can-and-cant-see)
- [Quick start](#quick-start) (Docker)
- [Running without Docker](#running-without-docker): a plain binary, optionally as a systemd service
- [Configuration](#configuration)
- [Putting it behind TLS](#putting-it-behind-tls)
- [Connecting Smaragd](#connecting-smaragd)
- [Operating it](#operating-it): backups, upgrades, limits, revoking devices
- [Maintenance](#maintenance): what the server tidies up by itself, and the admin commands
- [Building from source](#building-from-source)
- [Troubleshooting](#troubleshooting)

## What the server can and can't see

**Can't see:** file names, folder structure, document text, project metadata —
everything is sealed with XChaCha20-Poly1305 under a key derived from *your
passphrase* (Argon2id) that never leaves your devices. The server also can't
undetectably swap one document's data for another's: every blob is bound to its
vault and document.

**Can see (unavoidable for a server):** that a vault exists, how many documents it
has (as opaque random ids), the size and timing of each update, which device made
it, and your IP address. Device tokens and pairing codes are stored only as SHA-256
hashes.

**Can do:** withhold or delete data (so keep Smaragd's backups on — sync is not a
backup), or refuse service. It can't read or forge your content.

**Your passphrase is not recoverable.** The server never has it. If you lose it,
the synced data can't be decrypted — by anyone.

## Quick start

First generate an **admin token** — you need it to create vaults (see
[Connecting Smaragd](#connecting-smaragd)). Generate it once, print it, and save it
somewhere safe such as a password manager:

```sh
TOKEN="$(openssl rand -hex 24)"
echo "$TOKEN"
```

Then, with Docker (image published to GHCR on each release, or build it yourself below):

```sh
docker run -d --name smaragd-sync --restart unless-stopped \
  -p 8080:8080 \
  -v smaragd-sync-data:/data \
  -e SMARAGD_SYNC_ADMIN_TOKEN="$TOKEN" \
  ghcr.io/ljantzen/smaragd-sync-server:latest
```

The token is fixed when the container is created, so it survives restarts and
reboots. When you recreate the container (for example to upgrade), pass the same
token again. Check it's up:

```sh
curl http://localhost:8080/v1/health
# {"status":"ok","version":"0.1.0"}
```

With Docker Compose, use [`docker-compose.yml`](docker-compose.yml) in this directory.
Write the token **once** into a `.env` file next to it — Compose reads that file on
every `docker compose up`, so the token stays the same across restarts, upgrades and
new shells:

```sh
echo "SMARAGD_SYNC_ADMIN_TOKEN=$(openssl rand -hex 24)" > .env
chmod 600 .env
cat .env        # note the token down
docker compose up -d
```

Keep `.env` out of version control; it is a secret.

To build the image yourself, from the **repository root**:

```sh
docker build -f crates/smaragd-sync-server/Dockerfile -t smaragd-sync-server .
```

The container runs as a non-root user (uid 10001), listens on plain HTTP port 8080,
keeps everything under `/data`, and has a built-in health check.

## Running without Docker

Docker is only a convenience: the server is a single self-contained binary (SQLite
is compiled in) with no runtime dependencies. It is built and tested on **Linux**.
macOS and Windows should work — the code has nothing Linux-specific — but are
**untested**; on those, Docker Desktop running the image above is the tested route.
There are no prebuilt binaries yet, so build it with a current stable Rust
([rustup](https://rustup.rs)) and a C compiler (for the bundled SQLite: e.g.
`build-essential` on Debian/Ubuntu, the Xcode Command Line Tools on macOS, the
Visual Studio Build Tools on Windows), from a checkout of this repository:

```sh
cd crates/smaragd-sync-server
cargo build --release --locked
# the binary: target/release/smaragd-sync-server
```

To try it out, run it in the foreground. Without Docker the defaults are to listen
on `0.0.0.0:8080` and keep the database in `./data` (created if missing), so set
`SMARAGD_SYNC_DATA_DIR` to somewhere permanent (`$TOKEN` as generated in the
[Quick start](#quick-start)):

```sh
SMARAGD_SYNC_ADMIN_TOKEN="$TOKEN" \
SMARAGD_SYNC_DATA_DIR="$HOME/smaragd-sync-data" \
  ./target/release/smaragd-sync-server
```

`Ctrl-C` stops it cleanly. If you're putting a reverse proxy on the same machine in
front of it (see [TLS](#putting-it-behind-tls)), listen on the loopback interface only
with `SMARAGD_SYNC_LISTEN_ADDR=127.0.0.1:8080`.

### As a systemd service

For a permanent install on Linux, run it under its own unprivileged user:

```sh
sudo install -m 755 target/release/smaragd-sync-server /usr/local/bin/
sudo useradd --system --home-dir /var/lib/smaragd-sync --shell /usr/sbin/nologin smaragd-sync

# Configuration, including the admin token, in a root-only file:
sudo install -m 600 /dev/null /etc/smaragd-sync.env
echo "SMARAGD_SYNC_ADMIN_TOKEN=$(openssl rand -hex 24)" | sudo tee /etc/smaragd-sync.env
echo "SMARAGD_SYNC_LISTEN_ADDR=127.0.0.1:8080" | sudo tee -a /etc/smaragd-sync.env
```

Add any other [configuration](#configuration) variables to that file as well. Then
create `/etc/systemd/system/smaragd-sync.service`:

```ini
[Unit]
Description=Smaragd sync server
After=network-online.target
Wants=network-online.target

[Service]
User=smaragd-sync
Group=smaragd-sync
EnvironmentFile=/etc/smaragd-sync.env
Environment=SMARAGD_SYNC_DATA_DIR=/var/lib/smaragd-sync
StateDirectory=smaragd-sync
StateDirectoryMode=0700
ExecStart=/usr/local/bin/smaragd-sync-server
Restart=on-failure
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

and start it:

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now smaragd-sync
curl http://127.0.0.1:8080/v1/health
journalctl -u smaragd-sync -f     # logs
```

systemd creates `/var/lib/smaragd-sync` for the database, and the journal takes care
of log rotation. The admin token stays the same across restarts because it lives in
`/etc/smaragd-sync.env`; `sudo cat` that file if you need it again.

## Configuration

Everything is an environment variable; there is no config file.

| Variable | Default | Meaning |
|---|---|---|
| `SMARAGD_SYNC_LISTEN_ADDR` | `0.0.0.0:8080` | Address to listen on (plain HTTP). |
| `SMARAGD_SYNC_DATA_DIR` | `./data` (`/data` in the image) | Directory holding all state. Mount a volume here. |
| `SMARAGD_SYNC_ADMIN_TOKEN` | *(unset)* | Lets whoever holds it create vaults while open registration is off. |
| `SMARAGD_SYNC_ALLOW_OPEN_REGISTRATION` | `false` | If `true`, *anyone who can reach the server* may create a vault. Leave off unless the server is private. |
| `SMARAGD_SYNC_VAULT_QUOTA_MB` | `1024` | Maximum ciphertext stored per vault. |
| `SMARAGD_SYNC_MAINTENANCE_INTERVAL_HOURS` | `6` | How often the built-in maintenance task runs (see [Maintenance](#maintenance)). `0` turns it off. |
| `SMARAGD_SYNC_EMPTY_VAULT_RETENTION_DAYS` | `30` | A vault with no devices left is deleted this many days after its last activity. `0` keeps such vaults forever. |
| `RUST_LOG` | `info` | Log level (`tracing` filter syntax). |

If open registration is off **and** no admin token is set, nobody can create a
vault; the server warns about this at startup.

The admin token is only checked when a vault is **created**; the server doesn't
store it. Changing it (or losing it and setting a new one) has no effect on
existing vaults and paired devices — only the new token works for creating vaults
from then on. To change it, recreate the container with the new value (with
Compose: edit `.env`, then `docker compose up -d`).

## Putting it behind TLS

The server speaks plain HTTP. Bearer tokens travel in every request, so for anything
beyond your own machine or a trusted LAN, terminate TLS in a reverse proxy.
(Your *content* stays end-to-end encrypted even without TLS, but tokens and the
metadata listed above would be exposed.)

**Caddy** (automatic certificates):

```caddyfile
sync.example.com {
    reverse_proxy localhost:8080
}
```

**nginx:**

```nginx
server {
    listen 443 ssl;
    server_name sync.example.com;
    # ssl_certificate / ssl_certificate_key ...

    client_max_body_size 16m;   # the server accepts blobs up to 8 MiB

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $remote_addr;
    }
}
```

**Serving under a path** (e.g. `https://example.com/smaragd/`): the server itself
always serves at `/v1/...`, so have the proxy *strip* the prefix — in Caddy,
`handle_path /smaragd/* { reverse_proxy localhost:8080 }` — and enter the path
(`smaragd`) in Smaragd's Sync settings.

**Rate limiting** is left to the proxy (e.g. nginx `limit_req`). Pairing codes are
single-use, expire after 10 minutes and carry ~59 bits of entropy, so brute-forcing
one through a rate-limited proxy is impractical.

## Connecting Smaragd

In Smaragd's Settings → Sync, enter the server's host, port and whether it uses TLS,
and choose an encryption passphrase (use the *same* passphrase on every device).
Then use **Sync This Project** to create a vault — this needs the admin token when
open registration is off — and **pair** other devices with the short-lived code it
shows. See the *Sync* chapter of the user manual for the full walkthrough.

To create a vault by hand (mostly useful for testing):

```sh
SALT=$(head -c16 /dev/urandom | base64)
curl -X POST http://localhost:8080/v1/vaults \
  -H 'content-type: application/json' \
  -H "x-admin-token: $SMARAGD_SYNC_ADMIN_TOKEN" \
  -d "{\"device_name\":\"my laptop\",\"kdf_salt\":\"$SALT\"}"
```

The full HTTP API is documented in the `smaragd-sync-protocol` crate
(`crates/smaragd-sync-protocol/src/lib.rs`, rendered by `cargo doc`).

## Operating it

### Backups

Everything is in `sync.sqlite3` (plus its `-wal`/`-shm` companions) in the data
directory. Either stop the server and copy the directory, or take a consistent
online copy:

```sh
# Docker (the image is minimal, with no sqlite3 inside; run this on the host against the volume):
sqlite3 "$(docker volume inspect -f '{{ .Mountpoint }}' smaragd-sync-data)/sync.sqlite3" \
  ".backup '/backups/sync-$(date +%F).sqlite3'"

# Without Docker (the systemd setup above):
sudo -u smaragd-sync sqlite3 /var/lib/smaragd-sync/sync.sqlite3 \
  ".backup '/tmp/sync-$(date +%F).sqlite3'"
```

Remember that the server only holds ciphertext: a backup of it is useless without
your passphrase, and your devices' own project backups are the ones that matter.

### Upgrading

Pull the new image and recreate the container — or, without Docker, rebuild the
binary from the new release, replace `/usr/local/bin/smaragd-sync-server` and
`sudo systemctl restart smaragd-sync`. The database schema migrates forward
automatically. Take a backup first — downgrading is not supported, and a
server refuses to open a database written by a *newer* version.

### Limits

- Each pushed update or snapshot is at most **8 MiB**.
- Each vault may store up to `SMARAGD_SYNC_VAULT_QUOTA_MB` of ciphertext; over that,
  pushes fail with `507`. Smaragd clients replace a document's old updates with a
  compact snapshot (`PUT .../snapshot`) once it has about 64 of them, which is how a
  vault's size is kept in check.

### Maintenance

The server tidies up after itself, so an instance can run for a long time without attention. About a minute after start-up and then every `SMARAGD_SYNC_MAINTENANCE_INTERVAL_HOURS` hours it:

- removes **expired pairing codes**;
- deletes **abandoned vaults** — vaults whose last device was removed ("Stop syncing this project" on the last device, or a revoke) and that have seen no activity for `SMARAGD_SYNC_EMPTY_VAULT_RETENTION_DAYS` days. A vault that still has a device is never touched, however quiet. The clients' own project files are unaffected; only the server's encrypted copy goes;
- **vacuums the database** when at least 32 MiB, and a quarter of the file, is unused. SQLite never shrinks its file on its own, so without this the file would stay at its high-water mark after vaults are deleted. A vacuum briefly blocks requests.

Smaragd clients also compact each document's history into a snapshot on their own (see Limits).

What it *doesn't* do for you: **backups**, **upgrades**, disk monitoring and log rotation (with Docker, `--log-opt max-size` is worth setting; under systemd the journal rotates on its own), and **removing the encrypted history of deleted files** — the server can't tell which documents are deleted, so that data stays until its vault is deleted.

#### Admin commands

For looking around and cleaning up by hand, the server binary has an `admin` mode. It works directly on the database, so it is safe to run while the server is up:

```sh
docker exec smaragd-sync smaragd-sync-server admin list
docker exec smaragd-sync smaragd-sync-server admin purge-empty --days 14        # only lists
docker exec smaragd-sync smaragd-sync-server admin purge-empty --days 14 --yes  # deletes
docker exec smaragd-sync smaragd-sync-server admin delete-vault <vault-id> --yes
docker exec smaragd-sync smaragd-sync-server admin vacuum
docker exec smaragd-sync smaragd-sync-server admin maintenance                  # one pass now
```

`list` shows every vault with its devices, documents, size and last activity, plus the database file's size and how much of it is reclaimable. Anything destructive only *shows* what it would do unless you add `--yes`.

Without Docker, run the same commands as the server's user and with the same data directory, so any files SQLite creates stay owned by that user:

```sh
sudo -u smaragd-sync SMARAGD_SYNC_DATA_DIR=/var/lib/smaragd-sync smaragd-sync-server admin list
```

### Devices and vaults

Each paired device has its own token, and any device can list or revoke the
vault's devices from Smaragd's Sync panel; a revoked token stops working
immediately. Deleting a vault removes all its data and every device's token. A vault whose last device leaves is removed automatically after the retention period (see [Maintenance](#maintenance)); `admin delete-vault` removes one right away.

## Building from source

The server is its own Cargo workspace (so its dependencies stay out of the desktop
app's lockfile):

```sh
cd crates/smaragd-sync-server
cargo build --release      # target/release/smaragd-sync-server
cargo test                 # unit + HTTP tests
SMARAGD_SYNC_ALLOW_OPEN_REGISTRATION=true cargo run   # a throwaway dev server in ./data

# End-to-end tests (real Smaragd client + engine against this server) live in a
# crate of their own, because they link the whole desktop app:
cd ../smaragd-sync-e2e && cargo test
```

To install and run it for real, see [Running without Docker](#running-without-docker).

## Troubleshooting

- **`403` when creating a vault** — open registration is off and the admin token was
  missing or wrong. To see the token a running container was started with:
  `docker inspect smaragd-sync --format '{{range .Config.Env}}{{println .}}{{end}}' | grep ADMIN_TOKEN`.
  Lost it entirely? Set a new one (see [Configuration](#configuration)); existing
  vaults are unaffected.
- **`401` from a device that used to work** — its token was revoked, or the vault was
  deleted.
- **"sync passphrase doesn't match this vault"** — reported by Smaragd, not the
  server: a device is using a different passphrase than the one the vault was
  created with.
- **`507`** — the vault hit its quota. Clients compact old updates on their own, but a vault that is full of current content needs more room: raise `SMARAGD_SYNC_VAULT_QUOTA_MB`.
- **Container marked unhealthy** — `docker logs smaragd-sync`; the health check calls
  `GET /v1/health` on the loopback interface.
