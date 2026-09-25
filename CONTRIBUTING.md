# Contributing to Smaragd

## License

Smaragd is licensed under the [GNU GPL-3.0-or-later](LICENSE).

## Contributor License Agreement

Before your first pull request can be merged, you need to agree to the
[Smaragd Individual Contributor License Agreement](CLA.md). In short: your
contribution stays under the GPL, and you additionally grant the maintainer
the right to relicense it (e.g. as part of a future commercial edition of
Smaragd). You keep full copyright over what you write.

A bot checks every pull request automatically. On your first PR, it will
reply asking you to agree — just post the following as a comment on the
pull request, and the bot will mark it signed:

> I have read and agree to the Smaragd CLA (CLA.md).

## Making a change

1. Fork the repository and create a branch for your change.
2. Keep pull requests focused — one logical change per PR.
3. Open a pull request against `main` describing what changed and why.

## Working on sync

The self-hosted sync feature spans four places, each with its own tests:

- `src/sync/` — the client core (CRDT documents, manifest, engine, crypto). Unit tests include multi-device simulations against an in-memory server, so most logic can be tested without a network: `cargo test --lib sync::`.
- `crates/smaragd-sync-protocol/` — wire types shared by client and server. `cargo test -p smaragd-sync-protocol`.
- `crates/smaragd-sync-server/` — the server. It is its **own Cargo workspace** with its own `Cargo.lock` (so its dependencies stay out of the app's flatpak-vendored lockfile): `cd crates/smaragd-sync-server && cargo test`, or `just server-check`.
- `crates/smaragd-sync-e2e/` — end-to-end tests of the real client against the real server. Also its own workspace: `just e2e`.

`just check` runs everything CI does across all of them. Building the server's Docker image needs the repository root as the build context: `just docker-build`.

## Reporting issues

Please use GitHub Issues for bug reports and feature requests.
