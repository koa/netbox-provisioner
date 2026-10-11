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

WLAN is the `/interface/wifi` stack only (`docs/wlan-generation.md`): CAPsMAN on the Wireless
LAN Group's controller, CAPs routed (they reach the controller at its loopback) with local
forwarding into the bridge NetBox puts their radios in. Anything model specific (ports, radio
profiles, which package a board's radios need: `wireless`, `wifi-qcom`, `wifi-qcom-ac`) belongs
in `hw_facts.rs`. A radio stack is only read, and its package only installed, where NetBox needs
it (`needed_radio_package`); a controller reads `WifiDeviceData` without radios, CAPsMAN being in
the base package since 7.13.

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

## Device access, setup and updates

The UI logs in over OIDC (`yew-oauth2` in the frontend, structure taken from the cable-editor);
the backend checks the JWT against `auth_issuer` and lets only `auth_users` in. Devices are
reached directly, or, with `management_proxy` set, all through one device's `/ip socks` (SOCKS5
with password; `device::proxied_stream` retries the sporadic "General SOCKS server failure" of a
first connection). Nothing is written without `allow_apply`; `apply` runs the diff as a script on
the device (`apply_mutations_via_script`), so it completes even when it changes the path the
connection takes, then diffs again: a script stops at its first error, so whatever is left is
what failed. A diff that never gets empty after an apply usually means a field RouterOS accepts
but doesn't report in that state (see the `k`/`o` notes in mikrotik-model's CLAUDE.md); compare
with the device's own `print` (`examples/print_device.rs`, read-only) and on a CHR before
changing defaults.

A device that doesn't answer gets its initial setup over `/tool mac-telnet` from a neighbor that
does (`device::bootstrap`, on the device page): `plan` picks a cabled port with an IPv4 (on the
port or its bridge) whose other end is reachable. MAC addresses are not kept in NetBox: the
operator picks the physical device from what the neighbor's `/ip/neighbor` shows on that port
(`BootstrapPlan.discovered`) or types its MAC; don't make anything depend on NetBox MACs.
`set_up` drives the neighbor's console over SSH
(`device::terminal`, russh; login flags `+ct400w` so long echoes don't wrap) through the factory
prompts (license, default-config notice, new password - each answered once, some versions print
`new password>` twice), sets password, address, API and a firewall accept, and takes the port out
of the factory bridge in the background. Factory bridges answer MAC-telnet only on the bridge's
MAC, which is also the one they announce. `connect` falls back to that bootstrap
address when the primary IP doesn't answer yet, through an SSH tunnel to the neighbor
(`terminal::Tunnel`, `direct-tcpip`; `device::Path::Neighbor`): API, re-diff and SCP all go
from the neighbor's address on the cable, so the device needs no route back before its first
apply brings OSPF. Every generated config has `/ip ssh forwarding-enabled=local` for that;
without it RouterOS never answers the forward (no refusal), hence the timeout in
`Tunnel::open`. Don't go back to static return routes on the device: with
`redistribute=static` they go out as /32s that beat the neighbors' connected networks. When the
mac-telnet session ends early, the following lines land on the neighbor's own console (this
once left setup routes on lab-np1), so every setup command is wrapped to do nothing where the
global `netboxProvisionerNeighbor` is set, which `set_up` sets on the neighbor only.

`device::update` brings RouterOS to the newest release of the channel picked in the UI
(long-term, stable, testing, development; `routeros_channel` preselects it), from
`upgrade.mikrotik.com/routeros/NEWESTa7.<channel>` (`NEWEST7.*` stopped at 7.12.1; testing and
development are pre-releases like `7.25rc1`, which `Version` keeps for the file names and orders
before the release). v6 goes to the newest v6 first, v7 gets `routeros` plus the
`needed_radio_package`; every other package is uninstalled (`/system/package/uninstall
numbers=<.id>`, at the same reboot), and a device at or above the target only gets its packages
in line. Then the RouterBOARD firmware: `.npk`
from download.mikrotik.com, SCP upload (`terminal::upload`), size check (the size settles after
the copy; on 16 MB devices the root directory is in RAM, so a 12 MB package fits), reboot,
version check. A reboot is detected by the uptime going down, not by the connection failing:
connections may just wait while the device boots. RouterOS before 6.43 needs the API's MD5
challenge login (in mikrotik-api), and v6 reports some boards without `RB` (`912UAG-2HPnD`), so
`hw_facts.rs` lists both spellings. Entries added without a field may not report it (a firewall
rule without `disabled`, the OSPF instance an update from v6 brings without `vrf`); that is
`default=` in mikrotik-model. RouterOS 6 SSH offers only `hmac-sha1`/`-md5`,
`diffie-hellman-group14-sha1` and a 2048 bit group exchange, so `terminal` appends those to
russh's defaults and lowers the group minimum to 2048. A user with SSH keys can't log in over SSH
with a password while `/ip ssh always-allow-password-login=no` (7.24:
`password-authentication=yes-if-no-key`), so the update switches it for the upload only.

A `dhcp-client` on an `ospf` interface takes no default route (`add-default-route=no`; RouterOS
then doesn't report `default-route-distance`): the default comes over OSPF, and one from DHCP
would be redistributed although it only works from that device. A client network's (`dhcp`)
IPv6 /64 is advertised for SLAAC.

DNS servers come from the device's NetBox config context (`dns_servers`, in priority order):
client networks get them by DHCP (v4) and RDNSS (v6, only on devices with client networks),
every device asks them itself, and none resolves for others. Without the context, a router with
a DHCP server answers DNS (its networks get its own address, DNS dropped on `input` from a `nat`
uplink), and one without a `dhcp-client` forwards to the loopbacks of the `nat` routers.
`mu; ordered` makes a list field a `Box<[V]>` whose order counts (`dns-server`, `servers`).
An uplink tagged `nat` gets a masquerade rule. The provisioner owns
`/ip/firewall/filter` and `nat` completely (rules it doesn't generate are removed) and doesn't
reorder rules yet.

A WLAN with a VLAN ends where NetBox puts its network (e.g. the controller, over the VLAN
group's VXLAN). The CAP sets the VLAN, so a group's WLANs share one: with `wifi-qcom` its radios
get a datapath with `vlan-id` into the bridge carrying that VLAN; `wifi-qcom-ac` radios can't tag
(MikroTik: "802.11ac chipsets do not support this type of VLAN tagging"; clients associate but
get no DHCP), so they are static access ports of that bridge instead
(`RadioPackage::tags_vlans`). A device that is no controller or CAP anymore still
gets `WifiDeviceData` read while CAPsMAN/CAP are on or the certificates CAPsMAN generated (named in
`/interface/wifi/capsman generated-*`, not recognizable by trust store) are left, and these are
removed. A VLAN interface on a tagged bridge only exists where the device has L3 in that VLAN.

A device that powers another one over PoE (NetBox `poe_mode=pse` on the port cabled to it) must
not reboot while that one updates: uploaded packages install at the next boot, and losing power
then has bricked devices. `device::power::PowerChain::of` collects the devices up and down the
PoE chains; the update holds an update lease from before the upload to the end and a reboot
lease per reboot, and a reboot waits while a consumer holds an update lease, an update waits to
start while a supplier reboots (both under one lock). The leases are in-process only.

`local/` runs the lab: `netbox/` (NetBox + `seed-lab.py`), `keycloak/` (realm `lab` on
`localhost:8180`, admin/admin), `lab/make-config.sh` (the lab devices' password comes from the
ignored `.lab-netbox/device-password`, never put it in the repo) + `lab/run-provisioner.sh`
(builds frontend and binary, serves on :8082), `screenshots/` (Playwright, `run.sh STEP...`; a page reload asks
for the Keycloak login again, so navigate with `click:` instead of `goto:`). The harness's
`mac_telnet.rs` and `update.rs` cover setup and update against CHRs, reaching the new node
through a tunnel to the entry like the provisioner: `ensure_factory_image` gives
an untouched image of a `ChrRelease` (`CHR`, or `CHR_OLDER` to update from), `start_node` boots
it without waiting for the API, and `mac_of(node, slot)` is the MAC of a cabled NIC (slot 0 keeps
QEMU's, the golden image's dhcp-client is bound to it).

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
