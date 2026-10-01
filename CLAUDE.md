# netbox-provisioner

Cargo workspace (`binary`, `backend`, `frontend`, `netbox-model`) that provisions a MikroTik
RouterOS network from NetBox. `backend` fetches topology via GraphQL (`netbox-model`/`cynic`),
builds an in-memory `Topology`, generates per-device RouterOS config (`device/ros/`) against
`mikrotik-model` (sibling repo, path dependency: `../mikrotik-model`), and exposes it over a
GraphQL API + embedded `frontend` (Rust/WASM, built via `trunk`) through `binary`.

Building `binary` requires `trunk` and the `wasm32-unknown-unknown` target installed locally
(its `build.rs` runs `trunk build --release` against `../frontend`) — `backend`/`netbox-model`
alone do not need this and can be checked/tested independently.

The full architecture, current gaps, and the phased completion plan (NetBox data model, BGP/OSPF
design, apply/diff/review pipeline, etc.) are tracked outside this repo in the project plan; see
the `examples/` directory here for reference RouterOS configurations the generator must remain
capable of producing the functionality of (not byte-for-byte).

## Commits

- One focused commit per logical change; split unrelated work (e.g. a refactor and the feature
  built on it) into separate commits.
- Subject: short, lowercase, imperative, no trailing period (e.g. `finish l2 bridge planning`).
- Body: why the change was needed and anything non-obvious, wrapped at ~72 columns.
- Update this file in the same commit as the change it describes.
- Commits by Claude end with the `Co-Authored-By: Claude …` trailer, without a session link.
- Push to `master` only when asked — once CI is set up here, a push to `master` will publish a
  Docker image and Helm chart, so this matters more here than in the other two repos.
