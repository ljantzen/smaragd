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

The self-hosted sync feature is experimental (its formats and protocol may still change without migrations). It spans this repo and two others:

- `src/sync/` — the client core (CRDT documents, manifest, engine, crypto). Unit tests include multi-device simulations against an in-memory server, so most logic can be tested without a network: `cargo test --lib sync::`. This repo depends on `smaragd-sync-protocol` as a git dependency (see root `Cargo.toml`).
- [smaragd-sync](https://github.com/smaragd-editor/smaragd-sync) — the wire protocol crate and the server, in one workspace. See its own README/CONTRIBUTING for how to work on those.
- [smaragd-sync-e2e](https://github.com/smaragd-editor/smaragd-sync-e2e) — end-to-end tests of the real client against the real server. See its own README.

`just check` runs this repo's own CI checks (fmt-check, clippy, test); the other two repos each have their own CI.

## Reporting issues

Please use GitHub Issues for bug reports and feature requests.
