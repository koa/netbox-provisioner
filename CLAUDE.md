# netbox-provisioner

Cargo workspace (`binary`, `backend`, `frontend`, `netbox-model`) that provisions a MikroTik
RouterOS network from NetBox. `backend` fetches topology via GraphQL using the `graphql_client`
crate directly against `backend/src/netbox/schema.graphqls` + `fetch_topology.graphql` (the
`#[derive(GraphQLQuery)]` in `backend/src/netbox/mod.rs` generates the request/response types from
those two files at compile time) — edit `fetch_topology.graphql` to change what's fetched, no
other codegen step needed. `netbox-model` is a separate, currently-unused `cynic`-based NetBox
client that `backend` does not depend on; don't confuse its hand-maintained query structs
(`netbox-model/src/graphql/netbox.rs`) with the real, graphql_client-generated ones `backend`
actually uses. `backend` builds an in-memory `Topology`, generates per-device RouterOS config
(`device/ros/`) against `mikrotik-model` (sibling repo, path dependency: `../mikrotik-model`), and
exposes it over a GraphQL API + embedded `frontend` (Rust/WASM, built via `trunk`) through
`binary`.

A GraphQL scalar (e.g. NetBox's `JSON`) resolves to whatever Rust type of the exact same name is
in scope at the `#[derive(GraphQLQuery)]` site in `backend/src/netbox/mod.rs` — since that's keyed
by scalar name, not by field, every `custom_field_data`/`custom_fields` selection anywhere in
`fetch_topology.graphql` shares the one `JSON` struct there; add new custom-field keys to that same
struct rather than creating a new scalar-backing type per query site.

Building `binary` requires `trunk` and the `wasm32-unknown-unknown` target installed locally
(its `build.rs` runs `trunk build --release` against `../frontend`) — `backend`/`netbox-model`
alone do not need this and can be checked/tested independently.

The full architecture, current gaps, and the phased completion plan (NetBox data model, BGP/OSPF
design, apply/diff/review pipeline, etc.) are tracked outside this repo in the project plan; see
the `examples/` directory here for reference RouterOS configurations the generator must remain
capable of producing the functionality of (not byte-for-byte).

`backend/tests/chr_network/` is a multi-CHR network test harness: boots real MikroTik CHR
instances under QEMU (software emulation, no KVM needed) and proves actual distributed network
behavior, not just that the generator's output parses. `--ignored` by default (`cargo test -p
backend --test chr_network -- --ignored`) since it needs `nix` on PATH and takes real wall-clock
time to boot VMs; every test in it skips cleanly with a message if `nix` isn't available, and it
must never slow down or break a normal `cargo test` run. `golden_image.rs` builds and caches
(default `../.chr-test-cache/`, override via `CHR_TEST_CACHE_DIR`) a disk image with the license
accepted and the admin password set once; every actual test run is meant to boot a disposable
`qcow2` overlay on top of that image so a hard `kill -9` teardown is always safe.

## Commits

- One focused commit per logical change; split unrelated work (e.g. a refactor and the feature
  built on it) into separate commits.
- Subject: short, lowercase, imperative, no trailing period (e.g. `finish l2 bridge planning`).
- Body: why the change was needed and anything non-obvious, wrapped at ~72 columns.
- Update this file in the same commit as the change it describes.
- Commits by Claude end with the `Co-Authored-By: Claude …` trailer, without a session link.
- Push to `master` only when asked — once CI is set up here, a push to `master` will publish a
  Docker image and Helm chart, so this matters more here than in the other two repos.
- Verify with `cargo clippy` in addition to `cargo check`/`cargo test` — not every pre-existing
  warning needs fixing, but a change shouldn't leave new, avoidable ones behind.
