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

WLAN is mainly the `/interface/wifi` stack (`docs/wlan-generation.md`): CAPsMAN on the Wireless
LAN Group's controller, CAPs routed (they reach the controller at its loopback) with local
forwarding into the bridge NetBox puts their radios in. An AP with `wlan<N>` ports in NetBox
(`wireless` package, `Topology::is_legacy_cap`) is a CAP of the old `/caps-man` on the same
controller instead (`CapsManDeviceDataTarget`, the controller then needs `wireless` too). It
forwards its clients itself (`local-forwarding=yes`, the CAP's `bridge`), into the WLANs' VLANs
and their VXLAN like a CAP of `/interface/wifi`, so it is a VTEP; RouterOS hangs master and
virtual radios into that bridge with the `pvid` of their WLAN (tagged members of a dynamic bridge
VLAN entry). A CAP only gets a new WLAN when its radios are provisioned again, so an apply on a
controller that changes a CAPsMAN's WLAN definitions provisions its CAPs' radios again after
it (`reprovisioned_menus`, the job event `RadiosReprovisioned`; their clients reconnect). An
AP without radios in NetBox keeps its `/interface/wireless/cap` as it is. A wireless radio the board
doesn't have (a miniPCIe card, `wlan2` of an RB912) is taken from the device. Anything model
specific (ports, radio profiles, which package a board's radios need: `wireless`, `wifi-qcom`,
`wifi-qcom-ac`) belongs in `hw_facts.rs`. A radio stack is only read, and its package only
installed, where NetBox needs it (`needed_radio_package`); a controller reads `WifiDeviceData`
without radios, CAPsMAN being in the base package since 7.13.

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
the backend checks the JWT against `auth_issuer` and lets only `auth_users` in, both as
middleware of `/graphql` in `binary` (`authenticated_graphql`: `403` otherwise, and the server
log says why; the frontend words the status, `i18n::error::refused`) (`docs/login.md`:
the `auth_*` settings and how to set up Pocket-ID; keep it in line with them). The UI picks a
NetBox site first, then one of its devices (`/sites`, `/site/<id>`, `/site/<id>/device/<id>/show`,
`pages/routes.rs`): no sidebar, a breadcrumb of dropdowns to the sibling sites and devices
(`components/menu/`, page layout and `PopupMenu` taken from the cable-editor). Devices are
reached directly, or, with `management_proxy` set, all through one device's `/ip socks` (SOCKS5
with password; `device::proxied_stream` retries the sporadic "General SOCKS server failure" of a
first connection); the proxy's own device is reached at the proxy's address
(`bootstrap::reachable_address`), its primary IP may only exist once its config is applied.
Nothing is written without `allow_apply`, checked by the `WritesAllowed` guard of every
mutation that writes (the UI asks `allowApply` and disables apply, setup, update and reboot
with a hint instead of failing on the click); `apply` runs the diff as a script on
the device (`apply_mutations_via_script`), so it completes even when it changes the path the
connection takes, then diffs again: a script stops at its first error, so whatever is left is
what failed. That error comes back from `/system/script/run` (`ApplyRunning.error`) with the lines
before it applied, so it is no reason to take the fallback away early (it once was, on lab-core).
Before the script the device saves a backup and schedules loading it after
`apply_fallback_seconds` (180; `device::ros::fallback`); reaching the device again, even with a
part left, takes both away, otherwise it reboots into the config from before (CHR test
`fallback_is_stopped_or_restores`). A diff that never gets empty after an apply usually means a field RouterOS accepts
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
apply brings OSPF. It tries every neighbor of `bootstrap::plans` in turn, the management
proxy's device first: another neighbor may be cut off itself. Every generated config has `/ip
ssh forwarding-enabled=local` for that; without it RouterOS never answers the forward (no
refusal), hence the timeout in `Tunnel::open`. Don't go back to static return routes on the device: with
`redistribute=static` they go out as /32s that beat the neighbors' connected networks. When the
mac-telnet session ends early, the following lines land on the neighbor's own console (this
once left setup routes on lab-np1), so every setup command is wrapped to do nothing where the
global `netboxProvisionerNeighbor` is set, which `set_up` sets on the neighbor only. Nothing
in a console session waits a fixed time or takes a prompt for the end of a command (one is in
the echo of `[find]] > 0`): `Terminal::sync` types a numbered `:put ("in" . "sync" . n)` and
waits for `insyncn`, which comes after everything typed before. A device answering late (busy,
just booted; on a CHR stopped with `RunningNode::stall`) makes `/tool mac-telnet` give up
after `Connected` (only `Welcome back!`), hang until Ctrl-C, say "incorrect username or
password" for the right password, or end the session between commands, so only a session that
is up decides a login, every pair of credentials and MAC is tried again until
`LOGIN_TIMEOUT`, and after a session ending early all commands run again (each does nothing
where it is done; `mac_telnet_setup_survives_a_stalled_device`). The port leaves the factory
bridge once `/user active` has no mac-telnet session left, not after a delay. A home AP's
factory ether1 is WAN (firewall, no MAC server or discovery there), so it is set up after a reset
to CAPs mode (`docs/netbox-modeling.md`); `set_up` turns the CAP that mode enables off again
(`:parse`, the menus depend on the packages), since CAPsMAN refuses the first apply renaming the
radios.

`device::update` brings RouterOS to the newest release of the channel picked in the UI
(long-term, stable, testing, development; `routeros_channel` preselects it, long-term if unset), from
`upgrade.mikrotik.com/routeros/NEWESTa7.<channel>` (`NEWEST7.*` stopped at 7.12.1; testing and
development are pre-releases like `7.25rc1`, which `Version` keeps for the file names and orders
before the release). v6 goes to the newest v6 first, v7 gets `routeros` plus the
`needed_radio_package` (or `wireless` where a legacy CAPsMAN NetBox doesn't know of runs,
`/caps-man/manager enabled=yes`: the config leaves it alone, so the update must too); every
other package is uninstalled (`/system/package/uninstall
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

An update, an apply, an initial setup and a reboot run as jobs (`backend::job`):
`updateRouteros`, `applyCfg`, `bootstrap` and `rebootDevice` return the `Job` at once, the work
goes on in the background and reports typed events (`job::Event`, a GraphQL union; the
frontend words them in German, `i18n::job`, shown by `components/job.rs`). The frontend asks `job(id) { events(after)
}` every second: the API is plain POST with the bearer token, a subscription would need the
token over a websocket. A device runs one job at a time, the device page picks up a running one
(`jobs(deviceId)`); jobs are kept in the process only. Their numbers start anew per run
(`job::first_id`, from the start time), so after a restart the page asking for a job of the run
before gets `null`: it shows the job as interrupted (`components/job.rs`, `JobMessage::Lost`) and
frees the page. While asking fails (server away) it shows one line and asks again, less often. A panic in a job's work ends it as
failed with `PANIC` and its message, rather than leaving it running forever (`job::start`).
A command on an API connection that is gone answers `ConnectionClosed` (mikrotik-api; it
used to panic in `send_command`). `cancelJob` sets a flag the work checks
between steps (`Log::check`) and while it waits (`Log::unless_cancelled`), so a step that must
not stop halfway (the upload with `/ip ssh` switched) completes first. The update downloads all
packages of a step before the first upload, and removes the uploaded ones when it stops before
the reboot: a part of them must not install at some later boot. An apply stops before its
script, or after it while it waits, leaving the fallback, which brings the config from before
back. A setup stops before or after its console session (whose output streams as
`ConsoleOutput`, `Terminal::stream_to`), not within it. A reboot (`update::reboot`, "Neu starten" on the
device page, asked to confirm) takes the reboot lease like an update's and waits the same way.

NetBox's interface `mtu` is the L3 MTU of an Ethernet port or `wlan` radio (`setup_mtu`, and the
wireless target); `l2mtu` is raised to carry it with a VLAN tag, other interface types refuse
one. Both ends of a cable need the same (`port_mtu`): RouterOS 7's OSPF stays in ExStart/Exchange
between different MTUs and has no `mtu-ignore`, so a link is cut until both ends have it. A
rollout therefore goes outward from the management proxy's device: `connect` reaches a device
cut off from OSPF through the neighbor already changed (see the bootstrap fallback below), and
its apply brings the link back. A new MTU
restarts a `wlan` radio; on a DFS weather radar channel (5600-5650 MHz) it listens for radar for
10 minutes (ETSI) before it sends again. The lab runs 2000 on every link between its devices.
`DeviceCfg::generate_from` (the real path) calls the target's steps itself, not
`BaseDeviceDataTarget::generate_from` (which only tests use): a step added to one must be in the
other too. PoE out (`setup_poe`) was only in the latter, so the real path never set it before.

A `dhcp-client` on an `ospf` interface takes no default route (`add-default-route=no`; RouterOS
then hides `default-route-distance` and can't clear it, so it gets RouterOS' default 1, which
mikrotik-model reads for the hidden field): the default comes over OSPF, and one from DHCP
would be redistributed although it only works from that device. OSPF originates a default only
on a device with the custom field `propagate_default_route` (`originate-default=if-installed`,
RouterOS' own default), every other one gets `never`: an apply of the lab's uplink router, whose
DHCP default from the production network was still installed, once announced it back into
production. A client network's (`dhcp`)
IPv6 /64 is advertised for SLAAC. IPv6 is on everywhere (`/ipv6/settings disable-ipv6=no`):
with it off the addresses are `invalid` and OSPFv3 never comes up, while the diff looks applied.

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
group's VXLAN). With one VLAN in the group the CAP sets it: with `wifi-qcom` its radios get a
datapath with `vlan-id` into the bridge carrying that VLAN; `wifi-qcom-ac` radios can't tag
(MikroTik: "802.11ac chipsets do not support this type of VLAN tagging"; clients associate but
get no DHCP), so they are static access ports of that bridge instead
(`RadioPackage::tags_vlans`). With several (`WlanVlans::PerWlan`) the controller's configurations
carry each WLAN's `datapath.vlan-id` and the CAP's datapath only the bridge (MikroTik's CAPsMAN
VLAN example; no 802.11ax device in the lab to try it); a `wifi-qcom-ac` CAP is refused then,
it runs `wireless` instead with `wlan<N>` in NetBox (`hw_facts::runs_wireless`, the wAP ac).
A CAP's radio refers to a local `/interface/wifi/configuration` (`manager=capsman`, the
datapath; `capsman` for the first datapath, `capsman-<bridge>` for others) and keeps its
default name (`wifi1`; L2 names it so too): CAPsMAN binds it through that configuration. A
bound radio is flagged dynamic and RouterOS refuses any change to it, also with the CAP off
(hAP ax², 7.23.8: `can not change dynamic interface`), so it stays as read (`bound_radios`,
from `InterfaceWifiState::dynamic`), the configuration and datapath it uses stay, and a
different configuration than NetBox's is a warning before applying (`RadioBound`). What changes is the
configuration it uses (`datapath=`), a plain entry. A bound radio reports
`configuration=capsman` also where no such entry is (cAP ax, 7.23.7), which RouterOS provides
(`provided_by_routeros`, the sort's seeds besides `lo`, `main` and the firewall chains).
Moving to `wireless` leaves the `/interface/wifi` radios inactive under their names, so without a
wifi package they get their default names back, and `/interface/wifi` mutations come before
`/interface/wireless` ones (the sort keeps the order within a round). "Pakete anpassen" next to the
channel brings the packages in line without changing the version (`updateRouteros` without
`channel`). A device that is no controller or CAP anymore still
gets `WifiDeviceData` read while CAPsMAN/CAP are on or the certificates CAPsMAN generated (named in
`/interface/wifi/capsman generated-*`, not recognizable by trust store) are left, and these are
removed. A VLAN interface on a tagged bridge only exists where the device has L3 in that VLAN.

A device that powers another one over PoE (NetBox `poe_mode=pse` on the port cabled to it) must
not reboot while that one updates: uploaded packages install at the next boot, and losing power
then has bricked devices. `device::power::PowerChain::of` collects the devices up and down the
PoE chains; the update holds an update lease from before the upload to the end and a reboot
lease per reboot, and a reboot waits while a consumer holds an update lease, an update waits to
start while a supplier reboots (both under one lock). The leases are in-process only.
A modeled port of a PoE board without `pse` gets `poe-out=off`, which cuts what it feeds and
which the script alone doesn't make obvious, so `generateCfg` returns `warnings` next to the
script (`device::ros::warning::ApplyWarning`, a union `i18n::device::warning` words; `PoeCut` from
`BaseDeviceDataTarget::poe_warnings`: on at the device, off in the target, and
powering something: `/interface/ethernet/poe/monitor` reports `poe-out-status=powered-on`, not
`waiting-for-load`; read only where a port would lose PoE, and when it can't be read every such
port warns), and the device page shows them above the changes.

mac-telnet for remote maintenance: the generated config has an interface list `mac-server`
and sets `/tool mac-server allowed-interface-list` to it (`setup_mac_server`, after
`setup_l2`, which names the ports). The list holds the ports cabled to a device with RouterOS
in NetBox, the radios of a wireless link, and every free Ethernet port: not in NetBox, or there
without cable, address, VLAN and bridge, so a notebook on a free port on site can get in. It
takes the port itself, which RouterOS honors for a bridge port too (CHR 7.24.5), so customer
ports in the same bridge stay out. Interface lists are modeled whole: an apply removes every
list NetBox doesn't give, the factory `LAN` included. That is what left lab devices with
`allowed-interface-list=*2000011` before.

The device mode (`/system/device-mode`, RouterOS 7.17+) locks features the provisioner needs
(`device::device_mode::wanted`: `scheduler` for the apply fallback, `socks` on the management
proxy's device, `partitions` where `/partitions` reports 128 MiB or more for the planned update
fallback, `docs/concepts/update-fallback.md`). A change needs a confirmation at the device
(button, or a cold boot) while `/system/device-mode/update` counts down; closing its connection
cancels it and a warm reboot doesn't confirm (CHR 7.24.5), so the `DeviceMode` job holds the
connection until the device goes down, sends only features that are off (an update changing
nothing counts down too), and is done when the device mode reads back with them on
(`device_mode_change_confirmed_by_a_cold_boot`, `RunningNode::power_cycle`).

With `partitions` on, the `Repartition` job (`device::partitions::repartition`) splits a single
partition of 128 MiB or more in two (`/partitions repartition partitions=2`: reboot, reformat).
It compares identity, files and the partition count after the reboot and reports missing files.
On lab-core (CCR2004, 7.23.8) it took about 80 s and kept the configuration, all files and the
OSPF neighbors; part1 is empty afterwards. A CHR has no `/partitions`, so there is no CHR test.

An update on a RouterOS 7 device with two partitions first copies the running RouterOS to the
other one (`device::update_fallback::arm`, `/partitions copy-to`). It then adds a scheduler
`netbox-provisioner-update-fallback` (`start-time=startup`). Its script waits
`update_fallback_seconds` (default 900) and, if its own entry is still there, activates the old
partition and reboots. A running script doesn't stop when its scheduler entry is removed, so the
script checks for the entry itself. With the fallback armed, the update waits for a reboot
until the device could be back from it. Back on the old partition is `UPDATE_ROLLED_BACK`.
Reached on the new partition, the scheduler is removed: no switching back once the provisioner
sees the device. Then the OSPF neighbors that were `Full` before have to be `Full` again within
3 min, else `OSPF_NEIGHBORS_MISSING`. The way back is the device page's "Von partN starten"
(`bootPartition`), never automatic. A device without two partitions updates as before and gets
an `UpdateWithoutFallback` event. Checked on lab-core (7.23.8 → 7.24.5 and back, both ways):
`copy-to` ends by itself after about 7 s. Booting `part1` brings back the old version with its
configuration and without the scheduler. The scheduler switching back by itself is untested.

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

## Code conventions

- A RouterOS command no mikrotik-model type covers (actions, monitors, a menu read for one
  field) goes through `device::raw` (`command`, `rows`, `field`), which turns a `!trap` into
  `Error::Refused` naming the command; don't write another loop over `SimpleResult`.
- The backend sends data, the frontend words it: no sentence for the user is built in the
  backend (no German there at all). What the UI shows is a typed GraphQL object or union with
  the values the text needs (`job::Event`, `ApplyWarning`), an enum for a case to tell apart
  (`FallbackLeftReason`), never a placeholder string or an `Option` whose `None` means a case.
- Errors reach the UI as `error_info::ErrorInfo` (`code`, `params`, English `message`): a
  resolver that can fail returns `Result<_, ApiError>` (it puts code and params into the
  GraphQL error's extensions; a plain `Error` would lose them), a job or event field holds an
  `ErrorInfo`, never `error.to_string()`. A new case is a variant of `Error` or `SetupError`
  with the values as fields and its code in `error_info.rs` (the match is exhaustive), never
  a sentence in a `Box<str>`: a string field holds a value (a URL, a file, what the device
  or a library answered). A command or anything else that may hold a password goes in with
  it replaced by `***` (`bootstrap::hidden`). The frontend words a code in
  `i18n::error::server` and falls back to `message` for one it doesn't know yet.
- Every text the UI shows is in `frontend/src/i18n.rs` (German), a module per page or
  component, a `const` or a `fn` taking the values; components write none inline, so a second
  language replaces that one file. Names RouterOS and NetBox use themselves (`RouterOS`,
  `long-term`) stay as they are. `FrontendError::message` is the one-line text for the user,
  its `Display` is for the log.
- Who may call what is decided at the boundary, once: the token and `auth_users` in `binary`'s
  middleware, `allow_apply` by `#[graphql(guard = "WritesAllowed")]` on each writing mutation
  (a new one needs it too). The work below (`apply`, `bootstrap`, `update`) doesn't ask again,
  and the frontend only hides or disables what the backend refuses anyway.
- Before adding a helper, look whether one exists (`grep` the crate); one used from several
  modules belongs in a module of its own, not in the first one that needed it.

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
