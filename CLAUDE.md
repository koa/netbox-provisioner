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
capable of producing the functionality of (not byte-for-byte). `docs/` describes how the
NetBox-to-RouterOS config generation itself works (interface naming, L2/bridge/VLAN generation,
OSPF, VXLAN, WLAN/CAPsMAN, point-to-point radio bridges) - read it before changing anything under
`device/ros/`.
`docs/netbox-modeling.md` is the user-facing contract for how devices must be modeled in NetBox
(tags, custom fields, interface names, VLAN groups and their VXLAN VNI); update it whenever `fetch.rs` or
`fetch_topology.graphql` changes what is read. Every custom field read (the `JSON` struct in
`netbox/mod.rs`) is also described in `docs/custom-fields.md`, with how to create it in NetBox.

`device/ros/netbox_example_test.rs` runs every RouterOS device of the stored production
response `docs/netbox-response-example.json` (maintained by the user from the live NetBox; never
edit or commit it yourself) through the whole generator, with each device given exactly the ports
it declares in NetBox since the response carries no board model. Devices that don't generate yet
are listed in `KNOWN_FAILURES` with the expected error; the test also fails when a listed device
starts working, so fixing a generator gap means dropping its entry. Scripts land in
`target/netbox-example/` for review. An empty bridge with an address is legitimate there (CAPsMAN
or OpenVPN add their ports at runtime), so don't treat it as a generator error.

`backend/tests/chr_network/` is a multi-CHR network test harness: boots real MikroTik CHR
instances under QEMU (software emulation, no KVM needed) and proves actual distributed network
behavior, not just that the generator's output parses. `--ignored` by default (`cargo test -p
backend --test chr_network -- --ignored`) since it needs `nix` on PATH and takes real wall-clock
time to boot VMs; every test in it skips cleanly with a message if `nix` isn't available, and it
must never slow down or break a normal `cargo test` run. `golden_image.rs` builds and caches
(default `../.chr-test-cache/`, override via `CHR_TEST_CACHE_DIR`) a disk image with the license
accepted and the admin password set once; `vm.rs`'s `boot_node` boots a disposable `qcow2` overlay
on top of that image per node (`kill -9`-safe, verified live), wiring NIC slots either to a
host-reachable hostfwd (slot 0, management), a cabled peer (`-netdev socket` listen/connect pairs),
or an idle placeholder. `scenario.rs`'s `boot_scenario` derives that wiring straight from a
`topology::test::TopologyBuilder` plus a `DeviceId -> hwconfig::DeviceType` board map - the same
builder type production fixtures already use - rather than a second scenario DSL; it only resolves
plain interface-to-interface cables (no patch-panel `FrontPort`/`RearPort` chains yet), and slot 0
(`ether1`) is always reserved for management, so scenario topologies must cable `ether2` and up.
`apply.rs` applies config to a target node through a designated entry node's `/ip socks` proxy
instead of dialing the target directly (RouterOS defaults `/ip socks` to SOCKS4, not SOCKS5 -
`enable_socks` must keep requesting `version=5`) - every other access in a scenario (setup,
`fetch_config` reads, final assertions) is allowed to go directly to a node's own hostfwd port;
only the actual apply-under-test call is meant to take this realistic, routed-only path.
`wait.rs`'s `wait_for_ospf_full` polls `/routing/ospf/neighbor/print` (no typed model exists for
it yet) for a given OSPF instance name - the generator always provisions both a `default-v2` and a
`default-v3` instance the moment any interface has OSPF enabled, regardless of whether IPv6 is
actually in play, so a v4-only scenario must wait on `default-v2` specifically rather than "every
neighbor". `ospf_chain.rs`'s `three_node_ospf_chain_apply_via_entry` is the harness's end-to-end
proof scenario (project plan Schritt 7): a 3-node chain `entry -- a -- target` (a direct 2-node
cable would degenerate the realistic path into a same-subnet relay that never exercises routing at
all). Each node's own generated config is bootstrapped directly against itself first (scenario
setup, not the access pattern under test - a real device's actual management path is never the
one being reconfigured by its own diff), then a config change to `target` goes through `entry`'s
SOCKS proxy and `a`'s OSPF-learned route. `DeviceCfg::fetch_with_board_model` exists because CHR
has no `/system/routerboard` at all (it isn't a physical RouterBOARD), so the board has to be
supplied explicitly instead of detected live - this is the first time anything in this harness ran
`fetch_config`/`generate_mutations`/apply against a live CHR rather than a synthetic fixture, which
surfaced several real RouterOS/CHR quirks now handled in `ospf_chain.rs`'s bootstrap step: CHR's
factory dhcp-client on ether1 (the harness's own management NIC, never part of a scenario's
declared topology) must not be removed by the generated diff, since doing so over the very
connection riding on it severs that connection mid-command and hangs forever; CHR's virtio-net
ports reject `bandwidth` on `/interface/ethernet/set` outright (a known CHR hardware-emulation
gap, alongside the already-documented lack of PoE/combo/SFP+); and scenario link subnets must
avoid `10.0.2.0/24`, which QEMU's "user" netdev backing every node's management NIC uses
internally (gateway `10.0.2.2`), so an interface address landing in it collides with the slirp
virtual router and silently breaks guest networking. OSPF neighbor state reaching `Full` doesn't
mean the route is in the FIB yet under this harness's software-emulated (TCG, no KVM) CHRs - the
realistic-path apply retries "General SOCKS server failure" against a deadline rather than failing
on the first attempt, same as the isolated two-node apply test's ARP warm-up.

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
