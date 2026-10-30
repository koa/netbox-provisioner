# netbox-provisioner

Cargo workspace (`binary`, `backend`, `frontend`, `i18n`) that provisions a MikroTik
RouterOS network from NetBox. `backend` fetches topology via GraphQL using the `graphql_client`
crate directly against `backend/src/netbox/schema.graphqls` + `fetch_topology.graphql` (the
`#[derive(GraphQLQuery)]` in `backend/src/netbox/mod.rs` generates the request/response types from
those two files at compile time) — edit `fetch_topology.graphql` to change what's fetched, no
other codegen step needed. `backend` builds an in-memory `Topology`, generates per-device RouterOS config
(`device/ros/`) against `mikrotik-model` (sibling repo, path dependency: `../mikrotik-model`), and
exposes it over a GraphQL API + embedded `frontend` (Rust/WASM, built via `trunk`) through
`binary`.

A GraphQL scalar (e.g. NetBox's `JSON`) resolves to whatever Rust type of the exact same name is
in scope at the `#[derive(GraphQLQuery)]` site in `backend/src/netbox/mod.rs` — since that's keyed
by scalar name, not by field, every `custom_field_data`/`custom_fields` selection anywhere in
`fetch_topology.graphql` shares the one `JSON` struct there; add new custom-field keys to that same
struct rather than creating a new scalar-backing type per query site.

Building `binary` embeds `frontend/dist`, which has to be built first (`trunk` and the
`wasm32-unknown-unknown` target; PatternFly from `frontend/node` via `npm install`): `build.sh`
and `local/lab/run-provisioner.sh` do both. `binary/build.rs` only checks that `dist` exists and
rebuilds when it changes — `backend` alone does not need this and can be
checked/tested independently.

`flake.nix` builds the same with Nix (`nix build .#image`: an OCI image of the binary and the CA
certificates, no shell; `nix flake check`: tests, the three clippy runs, no shell or interpreter
in the closure) and is what CI runs (`.github/workflows/ci.yaml`). It takes `mikrotik-model` and
`mikrotik-api` as flake inputs from GitHub (branch `claude-edition`) and puts them next to this
repository, where the path dependencies find them: a change there reaches the image only after it
is pushed and `nix flake update mikrotik-model` (or `mikrotik-api`) is committed here; locally
`--override-input mikrotik-model path:../mikrotik-model` builds with the workspace's. A change of
`frontend/node/package-lock.json` needs the new `npmDeps` hash in `flake.nix` (the build names
it). The Helm chart is `charts/netbox-provisioner`: every setting of `config.yaml` is a value
of its own (`settings`), the chart writes them to a ConfigMap without the secrets; a secret
(`netbox_token`, `netbox_write_token`, a credential's or the proxy's `password`) is a `value` or
a `secretKeyRef` and reaches the binary as `APP_SETTINGS__…` variable. A new setting in
`backend/src/config.rs` belongs in `values.yaml` and `templates/configmap.yaml` (a secret one in
`netbox-provisioner.secrets` of `_helpers.tpl`).

The full architecture, current gaps, and the phased completion plan (NetBox data model, BGP/OSPF
design, apply/diff/review pipeline, etc.) are tracked outside this repo in the project plan; see
the `examples/` directory here for reference RouterOS configurations the generator must remain
capable of producing the functionality of (not byte-for-byte). `docs/` describes how the
NetBox-to-RouterOS config generation itself works (interface naming, L2/bridge/VLAN generation,
OSPF, VXLAN, WLAN/CAPsMAN, point-to-point radio bridges) - read it before changing anything under
`device/ros/`. There `mod.rs` has the `mikrotik_model!` definitions and the base config
(`BaseDeviceDataTarget`), `radio/` the radios (`wireless`, `w60g`, `wifi`, `capsman`, shared
helpers in `radio/mod.rs`), `dhcp.rs` the DHCP pool, `l2/` the bridges and VLANs. What a
board's switch chips offload, port by port, is in `hw_facts::SwitchChips` (from MikroTik's
manual; every chip but a CRS1xx/2xx's offloads one bridge only). `L2Setup::for_switch_chips`
turns the bridges on chips that filter VLANs in hardware into VLANs of one bridge
(`into_one_bridge`) and `snoops_multicast` leaves snooping off where it would cost the
hardware; the generated examples don't know the board and keep their bridges.
What the provisioner does beyond generating has a doc of its own in `docs/` too (device page,
jobs, interface view, config helper, update fallback, watchdog, deployment; listed in
`docs/overview.md`), in German, describing what is built; update it with the code.
`docs/concepts/` holds only what isn't built yet; once a concept is, it moves to `docs/` as a
plain description and its open parts become tasks.
`docs/todo.md` lists the open tasks of all three repos (numbered, with effort and status);
the commit that finishes one moves it to `docs/done.md`, with date and commit subject, keeping
its number (a finished part likewise, the rest stays in `todo.md`).
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
it (`reprovisioned_menus`, the job event `RadiosReprovisioned`; their clients reconnect).
Both that and the longer fallback of a radio change (`restarts_radio`) count only what the
script has (`in_script`, as the site diff): the diff carries an update without fields for every
entry, and lab-core's firewall-only apply provisioned its CAPs again and waited for a radio. An
AP without radios in NetBox keeps its `/interface/wireless/cap` as it is. A wireless radio the board
doesn't have (a miniPCIe card, `wlan2` of an RB912) is taken from the device. Anything model
specific (ports, radio profiles, which package a board's radios need: `wireless`, `wifi-qcom`,
`wifi-qcom-ac`) belongs in `hw_facts.rs`. A radio stack is only read, and its package only
installed, where NetBox needs it (`needed_radio_package`); a controller reads `WifiDeviceData`
without radios, CAPsMAN being in the base package since 7.13. A 60 GHz radio is keyed by its
name (`/interface/w60g` reports no `default-name`), so it is never renamed: it stays `wlan60-<N>`.
A radio's bridge port (`wlan<N>`, `wlan60-<N>`) leaves `hw` unset: RouterOS doesn't report it there. A
`wlan<N>`/`wifi<N>` in NetBox on a 60 GHz board is `PortNotFound` (its misnamed `wlan60-<N>`).
The diff sees the device as it is after the renames of its ports and radios
(`following_renames`): a bridge port, list member or bridge VLAN follows a rename on RouterOS
(CHR 7.24.5). Paired by the old names, the bridge ports of relabeled ports each looked like
another's, one was set onto an interface another port still held and the script stopped
("device already added as bridge port", the device cut off by the VLANs it removed before).

`device/ros/test/netbox_example.rs` runs every RouterOS device of the stored production
response `docs/netbox-response-example.json` (maintained by the user from the live NetBox; never
edit or commit it yourself) through the whole generator, with each device given exactly the ports
it declares in NetBox since the response carries no board model. Devices that don't generate yet
are listed in `KNOWN_FAILURES` with the expected error; the test also fails when a listed device
starts working, so fixing a generator gap means dropping its entry. Scripts land in
`target/netbox-example/` for review. An empty bridge with an address is legitimate there (CAPsMAN
or OpenVPN add their ports at runtime), so don't treat it as a generator error.

A cable path (`CablePortAccess::walk_cable`, `InterfaceAccess::connected_interfaces`) goes
through patch panels (front port to rear port) and through circuits: a cable ending at a
`CircuitTermination` goes on at the circuit's other end (`peer`, only with exactly two ends), as
NetBox' trace does, so a fiber between buildings connects the devices at both ends. Older
exports lack `circuit_list` and the ids of the circuit ends; `example_topology` drops those
ends, so their cables end there as before.

`topology/findings.rs` computes the findings about NetBox itself (`docs/config-helper.md`,
Befund „NetBox in sich“) once per built topology (`Topology::with_findings`, called by
`build_topology_from` and `TopologyBuilder::build`; a test changing the topology after `build`
calls it again). Only RouterOS devices are checked; each finding is a typed variant of the union
`FindingKind` with refs (id + name) the frontend links into NetBox (`netboxUrl`, the web UI
derived from `netbox_url`) and words in `i18n::findings`. Of a cable, the MTU is compared at the
ports; `ospf` on one end and a common subnet at the port's bridge where it has one, and only when
both ends have IPv4: a radio bridge device's end is L2 only. `findings_of_netbox_example` writes
what the stored export has to `target/netbox-example/findings.txt`. RouterOS takes one DHCP server per interface
(CHR 7.24.5: "server or relay with such interface already exists"), so the IPv4 networks of a
`dhcp` interface share one: the first with a pool creates it under the name it always had
(`dhcp-<interface>[-<n>]`), a further one adds its ranges to that pool and gets a network entry
of its own. The server's lease time is the prefix's custom field `dhcp_lease_time` (seconds;
the shortest of the port's networks, none or 0 RouterOS's 30 min); mikrotik-model reads it as a
`Duration`, since RouterOS reports `120s` as `2m`. A `dhcp` interface whose prefix leaves no
address for a pool gets no DHCP server, silently; `DhcpPoolEmpty` says so, computing the pool
with the generator's `device::ros::dhcp_pool_v4`, unless another network of the interface has a
pool (its one server serves that; the `/31` next to a foreign radio bridge's DHCP network) (a single free address is a pool of one, it
once was lost, so a /30 customer port got no server). A /32 with `dhcp` (router-stall-burri's
`lo`) is the same finding without prefix and no server, not a generator error; an IPv4 with
`dhcp` and without its prefix is `NO_PREFIX`, naming the interface. IPv6 needs no prefix
there (SLAAC). A WLAN without VLAN is an error at its
group's controller and APs (`WlanWithoutVlan`): the CAPs bridge their clients by its VLAN and
carry them over its VXLAN, so without one the clients reach nothing (router-walter). A wireless link end without RF role is a warning (`RadioWithoutRole`): the radio then keeps its
configuration, no bridge is set up. `nat` without `internet` is a warning (`NatWithoutInternet`,
Z2: `nat` needs it). A primary IPv4 on `lo` without `ospf` on any interface is an error
(`LoopbackWithoutOspf`): nothing announces it, and it ends a radio bridge's DHCP fallback. A radio
bridge is an OSPF router through a static address and `ospf` on its `bridge` (management only,
forwarding stays L2). Pool ranges are written as RouterOS reports them
(`pool_range`: one address alone, a range covering exactly one network as that network, CHR
7.24.5), else the diff never gets empty. The prefix tags
`loopback`/`transfer` (`IpPrefixData::pool`) are only used here so far. One L3 gateway per VLAN
counts per domain (`topology/vlan_domain.rs`): with a VNI every RouterOS device with the VLAN,
without one the devices that cables carrying it join, so the same VLAN local on two devices is
two networks. Two gateways are an error (`VlanWithSeveralGateways`), none a warning
(`VlanWithoutGateway`), except a WLAN group's management VLAN, which CAPsMAN uses on L2. Tagged
VLANs on an Ethernet port (or its bridge) are a warning (`TaggedVlanOnPort`, Z26); a bridge
device's radio link missing a cable, the link or its SSID is `RadioPathIncomplete` (from the
row's `RadioPath`, once per link); a bridge device without `lo` is an error, it is reached at its
loopback only. A port cabled to a RouterOS device whose label doesn't name it (without case and
punctuation, either containing the other) is a hint (`LabelNotFarEnd`); the export has many.

`topology/interfaces.rs` sorts a device's interfaces by task (`DeviceAccess.interfaceView`,
`docs/interface-view.md`): loopback, uplink, radio link, customer, direct, unknown, one
row per L3 interface with a bridge's ports in it, the findings of `findings.rs` grouped onto the
rows by the interfaces they name (those about the device as a whole go with the loopback), never
computed again; a new check is a new finding. The order of the rules and what a bridge device is
(`DeviceAccess::is_bridge_device`) come from the stored export and the lab, see `docs/interface-view.md`;
`interfaces_of_netbox_example` writes the export's rows to `target/netbox-example/interfaces.txt`.
The device page shows them in `components::interfaces`, every port with a row of its own (a
bridge's ports below it, `InterfaceRow.portRows`, with PoE only; the free ones in a section
that opens) and on it exactly the actions possible there, the helper starting from that port:
there is no menu of all helpers. A free or unknown port gets the roles, an Ethernet port
without a router on its cable also „Als Richtfunk“ (`HelperPreset::RadioChoose`: the flow asks
for a bridge device with one radio and no cable to a router but to this port first, then embeds
its helper as `RadioFrom`), every port but loopback and uplink „PoE“ and „Flags“ (`ActionKind::Edit`,
`HelperPreset::Poe`/`Flags`: the PoE form with the port and its cable's far end, the flags of
that interface only; a form doesn't offer the port its preset gives, `HelperPreset::fixes_port`,
the modal's title names it); a customer bridge's port „Aus Bridge lösen“ (`helper::bridge_leave`:
`UpdatePort` with `leaves_bridge`, its own VLANs with it; the bridge stays, named in
`NetboxPlan.emptied_bridge` when it was the last port); a router's port without cable „Neues
Gerät anschliessen“ (`helper::new_device`, also on a port whose one cable path ends open at a
patch panel, `new_device::landing`: the new cable starts at that front or rear port,
`CreateCable.a_end`; a suggestion with the board and port when the live
check sees an unknown MikroTik there: `CreateDevice` with the router's site and tenant, platform
`routeros`, the device type and role chosen from `Query.newDeviceOptions`, the WLAN group of an
AP; the cabled port, which the write takes from the device type when NetBox made it; then
„Loopback“ and, but for a `wireless-bridge`, „Verbindung“ planned on the preview, whose steps refer
to the new objects by their preview ids from `u32::MAX` down, `PlanStep::with_real_ids` in the
write); an address the live check finds only on the device „<address> übernehmen“
(`interface_live`: the static rows of `/ip/address` and `/ipv6/address` but link-local ones
whose IP no NetBox interface of the device has, but those on interfaces of a type the provisioner
doesn't manage (`interface_live::hand_made_interfaces`, by `/interface` type: tunnels, PPPoE
clients), which the generator keeps as they are (`BaseDeviceDataTarget::keep_hand_made_addresses`,
by name: not an Ethernet port, bridge, VLAN, VXLAN, radio, `lo` or a removed `*N`), like BGP;
`LiveRow.device_only_addresses` at the row of
their NetBox interface, by port name or another address there, else `other_addresses`; offered
only when NetBox has the IP nowhere; `helper::address_takeover`: `CreateAddress` on that
interface, before it `CreatePrefix` of its network when NetBox has neither that prefix nor an
address in it and the length isn't a host's, within the smallest prefix holding it, scoped to
the device's site unless that one is a pool's); a static DHCP lease only the router has
„Lease <address> übernehmen“ (`interface_live::device_only_leases`: static rows of
`/ip/dhcp-server/lease` whose address isn't a lease NetBox gives the router, at the row of the
server's interface, else the one whose network holds it, else `other_leases`; offered for a
lease with a MAC whose address NetBox has nowhere; the name is `host-name` up to its first
control character, `client_name`: a Tinkerforge brick pads it with NULs; `helper::lease_takeover`:
`CreateDevice` without platform, `CreateInterface` `eth0` with the MAC, written as
`dcim/mac-addresses` and the interface's `primary_mac_address`, `CreateAddress`). Each row with its actions (one may be
another device's helper: „Funkbrücke“ from a router port is the cabled bridge device's,
`HelperPreset::RadioFrom`, so `HelperFlow` embeds that device's helper; „Richtfunk abräumen“,
`helper::radio_disconnect`, starts from the row's radio only, „Kunden-Anschluss abräumen“,
`helper::customer_disconnect`, from the row's interface; „VLAN am Port“, `helper::port_vlan`, is
the suggestion of a customer row with `VlanWithoutPort`, a VLAN interface with a network whose
VLAN no port, WLAN or VXLAN carries anywhere, and a role for an unknown Ethernet port; it offers
the VLANs by group, without those the L2 setup would refuse on the port: a VID another VLAN of
the device has, a tunneled group whose VXLAN is in another bridge than the port would be in, and
its plan runs `L2Setup` on the preview for the rest); its query
hands the findings on to the type of `Findings` by serializing them (`rows::findings`), so
`components::findings` words them. The live checks of the rows are `device/interface_live.rs`
(`AccessibleDevice.interfaceLive`, a query of its own after the view, grouped onto the same
rows, `DeviceAccess::interface_rows`): who the device sees (`neighbors::read`), its OSPF
neighbors there, and the radio link's bridge devices answering a ping (`raw::answering`). An
OSPF neighbor's interface is RouterOS' name, which for a bridge or VLAN interface is L2's
(`bridge-link` for NetBox' `link`), so it is matched by port name, else by the addresses
`/ip/address` has on it; never by NetBox' name alone. A live row also carries actions
(`LiveRow.actions`, shown before the row's own and replacing one with the same helper): where
the device sees exactly one router, no radio bridge device, at a port NetBox has no cable at,
„Verbindung“ with both ends; where NetBox's cable goes elsewhere (`CableElsewhere`,
`OtherFarPort`), „Kabel übernehmen“, `helper::cable_takeover`: „Verbindung abräumen“ at each
end with another link, one after the other on the preview, then „Verbindung“ on the result,
its steps shifted behind (`PlanStep::shift`); a cable over a patch panel is refused
(`PORT_PATCHED`), the new one would be direct.

`device/site_diff.rs` is the site page's „Abweichung von NetBox“ (`Mutation.diffSite`,
`components::site_diff`): a job of the site (`job::start_site`, `JobSubject::Site`; a site's job
locks none of its devices, a second start gives back the running one) reads its RouterOS
devices, four at a time, and emits `DeviceDiffed` per device with the changes of an apply by
menu (`AccessibleDevice::menu_changes`: the sorted mutations counted like the script, which
leaves out every update without fields) or the error. A job's `deviceId`/`deviceName` are
`null` for a site's job, `siteId`/`siteName` then say which.

`device/neighbors.rs` compares what a device sees at its ports (`/ip/neighbor`) with NetBox's
cables (`AccessibleDevice.neighborFindings`, read-only; the site page asks every device at once,
`components::neighbor_check`). Ports on the device and the ports neighbors report carry the names
the generated config gives them: match by NetBox's (default) name or the short name before the
first `-` (`s01-core` is `sfp-sfpplus1`), never by MAC. A cable to a radio bridge device can't be
seen (the router on the other side shows up instead). A `discover-interface-list` naming a list
that doesn't exist (`*2000011`, the factory `LAN` an apply removed) means discovery is off; the
generator sets it to `maintenance` now.

`helper/` holds the helpers that write to NetBox (`mod.rs` the plan and its preview,
`plan.rs` its steps, `write.rs` writing and undoing them; `docs/config-helper.md`:
`helper::loopback`, `helper::connection` for a direct link between two routers: cable if
missing, the next free `/31` of the transfer pool as a new prefix, an address at each end,
`ospf`, MTU, the far device as label), `helper::disconnect` taking a wrong link out of NetBox
(its cable, the port's transfer-pool addresses and the far end's in the same network, the
link's prefix once empty, label naming the other end and `ospf`; a primary IP is refused; the
plan names devices it cuts off, `NetboxPlan.cut_off`). „Verbindung“ offers it as a button for a
chosen port whose link is in the way. `helper::flags` switches the tags `ospf`, `dhcp`,
`dhcp-client`, `nat`, `internet` of a device's interfaces (`flagInterfaces`: all but bridge ports, which
come only with a tag to take off); `UpdateInterface` carries `tags_added`/`tags_removed`. Its card
shows the device's findings after the switches live: `topology.findings(deviceId:, after:)`
computes them on the preview of a helper's plan. `helper::customer_net` puts the next free
network of a chosen site prefix (`customerNetOptions.parents`: IPv4, scope the device's site, no
pool, not a network itself; `IpPrefixData.site`) on a free port or a VLAN (its virtual interface,
or a new one named like the VLAN, `CreateInterface.vlan`/`tags`): prefix scoped to the site,
gateway as the first address, `dhcp` if asked; candidates overlapping a route more specific than
the parent are passed over (`Routes::overlapping`). IPv6 likewise from a chosen IPv6 site prefix
(`parentsV6`, `parentV6`): the next free `/64`, `::1` as gateway, announced for SLAAC by `dhcp`.
Either family may come alone, so a client network (`dhcp`) with IPv4 but no IPv6 is offered too
(`open`) and gets its IPv6 later. `helper::poe` sets `poe_mode` `pse` and
`pd` on two Ethernet ports (`is_ethernet`, any device; `Interface.poe`, `UpdateInterface.poe`),
cabling them if both are free; `poeOptions` offers the device's ports and the site's devices,
`AccessibleDevice.poeObserved` what the device feeds now (`poe/monitor` `powered-on`, the far
end by NetBox's cable, else `/ip/neighbor`). Without the port fed (`PoeInput.pd` none) the
feeding port alone gets `pse`, PoE on before the far device is in NetBox; „Neues Gerät“ at a
`pse` port gives the new device's port `pd`. „Kein PoE“ (`Helper.poeRemove`,
`poe::plan_remove`, `UpdateInterface.poe_removed`, written as `poe_mode: ""`) takes a port's mode
off and the counterpart at its cable's far end. The live check of the rows compares the modes
with the device (`poe::mismatches`, `LiveRow.poe`, port by `default-name`): `pse` without
`poe-out`, `pse` with a cable and `waiting-for-load` (without one it waits for its device), `pd`
where it feeds; each port gets „PoE-Eintrag entfernen“ (`ActionHelper::PoeRemove`, shown at that
port only). A `pse` port feeding (`powered-on`) whose cabled far end isn't `pd` gets „PoE
vervollständigen“ instead (`PdMissing`, `ActionHelper::Poe` as a suggestion). PoE in isn't reported per
port, and devices without RouterOS aren't checked. `helper::radio_bridge` completes a radio bridge
between two routers on two existing devices (`radioBridgeCandidates`: one radio each): cable to
the router port where none goes to a router, bridge with ether and radio as ports
(`UpdatePort`: name, RF role, bridge; `leaves_bridge` takes it out of one), the RouterOS radio name, `lo` (`loopback::add_to`), the
wireless link (`CreateWirelessLink`/`UpdateWirelessLink`: SSID, WPA with a PSK generated only
where none is; the PSK is never in the fingerprint, a step's `Debug` or the log), and the
segment's IPv4 (the router port's network, else a new `/29` of the transfer pool) with an
address and `ospf` on each router port and bridge; IPv6 stays link-local, the bridges are
reached at their loopback. Two bridges without RouterOS (UISP Wave Nano, set up by hand with a
DHCP client; `RadioBridgeCandidate.routeros`, an Ethernet port by NetBox type; its 60 GHz
`wlan0` and 5 GHz backup `wlan1` are paired with the other's by link, else by name, and shown
by the 60 GHz one, `interfaces::main_radio`) get
`helper::foreign_radio_bridge` instead: cables, labels, RF roles, the wireless link (a PSK only
for a new one), a `/31` of the transfer pool between the router ports for OSPFv2 and on each
router port a `/29` with `dhcp` and `dhcp_lease_time` 120 s (`CreatePrefix.dhcp_lease_time`),
`ospf` and `dhcp` there; the preview generates for the AP side's router (`NetboxPlan.device`).
`next_free_network` skips the prefixes the plan creates already. Topology carries the link's id and peer (`Interface.wireless_link`,
`WirelessLinkEnd`); `example_topology` makes up both for an export lacking them. Deleting is undone by creating the object again from the
JSON NetBox had (`NetboxWritten.recreate`, `helper::write::recreate`; it gets a new id): undoing an
older write whose objects were deleted and created again since fails on their old ids. The GraphQL input `Helper` is a `@oneOf` (graphql_client
makes it an enum; its queries need `skip_serializing_none`), naming the device or the two ports;
the device page opens them in a modal (`components::helper_flow`, a stepper over the embedded
`NetboxHelper`, `embedded`/`preset`/`on_phase`) from a row of the interfaces card, which the
backend gives its actions (`InterfaceRow.actions`, `InterfaceView.loopbackActions`: kind, helper,
preset ports or VLAN); "Rückgängig" is the device's last write that wrote something, also one
that failed midway (`Job.netboxWrittenCount`): NetBox has no transaction across objects, so the
steps before the failure stay, and the helper says "Nur teilweise geschrieben" instead of
"Geschrieben".
`components::netbox_helper` keeps the component and what all helpers share in `mod.rs` and
`choice.rs` (presets, `HelperKind`, `HelperChoice`, `helper_input!`), the plan and preview in
`plan.rs`, and gives each helper with a form a module with its choice, form, queries and its
own message (`LinkMsg`, `PoeMsg`, …, wrapped by `From` so callbacks give it as is).
A `CableMissing` neighbor finding with both ends in NetBox opens the card below it with those
ends (`components::neighbor_check`, `LinkPreset`). Lists of devices and interfaces follow
NetBox' order (`topology::natural`, its `naturalize`/`naturalize_interface`).
A helper computes a `NetboxPlan` of steps (`PlanStep`: create interface, address, cable,
prefix, set primary IP, update interface or port, create or update wireless link, delete
cable, address, prefix) from the topology plus REST reads
(`netbox::rest::NetboxRest`; GraphQL can't list free addresses or write). A new address must also be unrouted:
`helper::routes::Routes` reads `/ip/route` and `/ipv6/route` of one OSPF router (the management
proxy's device, else the RouterOS devices with the most OSPF interfaces, first of three to
answer) and skips a candidate inside a route more specific than its pool (undocumented in NetBox, but in
use), listing it in `NetboxPlan.undocumented` (a `/31` also when NetBox has addresses in it
without a prefix); when no router answers, no address is handed out
(`NoRouteCheck`). Its `preview` is the topology rebuilt from the
NetBox response it was built from (`Topology.source`, `Topology::rebuilt_with`) with the plan
applied to that JSON, created objects getting ids from `u32::MAX` down; `previewAccess` connects
through the device as it is now (`AccessibleDevice::generating_for`) but generates from the
preview, since the new primary IP may not be on the device yet. Writing (`writeNetboxPlan`) is a
job: it recomputes the plan, refuses one whose `fingerprint` changed, checks before each step that
what it builds on is unchanged (no `lo` yet, address or prefix free, ports without cable, label,
MTU and primary IP as planned), and emits a `NetboxWritten` event per object with the old values; `undoNetboxWrite` replays those backwards
(lost on a server restart). Every write carries `changelog_message` (`<plan description>
(<user>)`); writing needs `allow_netbox_write` and `netbox_write_token` (`NetboxWritesAllowed`),
the lab config sets both. The topology is fetched again after each write or undo.

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
`topology::builder::TopologyBuilder` plus a `DeviceId -> hwconfig::DeviceType` board map - the same
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
(`components/menu/`, page layout and `PopupMenu` taken from the cable-editor). Selects use
`menu::popup::Select` on `PopupMenu`, not patternfly-yew's `SimpleSelect`, whose fixed-position
popper left the end of a long list out of reach. Devices are
reached directly, or, with `management_proxy` set, all through one device's `/ip socks` (SOCKS5
with password; `device::proxied_stream` retries the sporadic "General SOCKS server failure" of a
first connection); the proxy's own device is reached at the proxy's address
(`bootstrap::reachable_address`), its primary IP may only exist once its config is applied.
Nothing is written without `allow_apply`, checked by the `WritesAllowed` guard of every
mutation that writes (the UI asks `allowApply` and disables apply, setup, update and reboot
with a hint instead of failing on the click); `apply` runs the diff as a script on
the device, started in the background, so it completes even when it changes the path the
connection takes, then diffs again: a script stops at its first error, so whatever is left is
what failed. The script leaves marks in a file on the device as it goes (`device::ros::progress`,
`ScriptProgress`): each menu begun, its end or the line it stopped at with RouterOS' message, and
the lines after which a ping from the device to the provisioner (the address most of its API
sessions come from, behind the SOCKS proxy the proxy's) stopped or came back. The ping lags
about one line behind the change that cut it. The file is under `flash/` where the device has
that disk (its root is in RAM then, the marks were gone after a reboot on lab-w60-a), so the
marks outlast the fallback's reboot and say where an apply cut the device off. Before the script
the device saves a backup and schedules loading it (`device::ros::fallback`) after
`SCRIPT_LIMIT` (120 s) plus `apply_fallback_seconds` (90); the script's end (or its error)
brings it forward to `apply_fallback_seconds` from then. The apply reads the marks every few
seconds: once through and reached, it takes the fallback away (not within 10 s of it firing,
`fallback::DUE`, `FallbackLeft` `DUE`), then diffs. When it fires is counted from the uptime
the script's mark gives for bringing it forward (`fallback::Rearmed`): a scheduler counts by
uptime, and a clock jumping forward (NTP after the apply set the DNS servers) leaves `next-run`
behind; by `next-run` and the clock 1og-sw1's fallback looked due at once, stayed and took a
good apply back (CHR test `a_clock_jump_leaves_the_fallback_time`). Reached, even with a part left, the fallback goes; not reached, the apply
waits for the device to be back with the config from before (`FallbackFired`) and shows the
marks then (CHR tests `fallback_is_stopped_or_restores`, `marks_outlast_the_fallback`). A device
without marks of the run counts as through after 90 s, as before them. A diff that never gets empty after an apply usually means a field RouterOS accepts
but doesn't report in that state (see the `k`/`o` notes in mikrotik-model's CLAUDE.md); compare
with the device's own `print` (`examples/print_device.rs`, read-only) and on a CHR before
changing defaults. A plan that changes a radio (`/interface/wireless…`, `/interface/wifi…`,
`PlannedApply::radio_wait`) gets a fallback 24 min longer (`RADIO_RESTART`, shown in
`FallbackArmed.radioWait`, part of the window after the script): a 5 GHz AP with `frequency=auto` picks its channel again and on a
DFS channel touching the weather radar band listens 10 min before it sends (lab-sxt-a: a new
SSID and key on 5680 MHz at 80 MHz, the link back after 10.5 min; before, the fallback fired
first). Radar meanwhile moves it to a channel that listens again (berghof-haus-stall: radar on
5680 MHz 5 min after the apply, then 5560 MHz), so the wait covers two of them. The high
channels stay, they are Switzerland's outdoor channels with the most power. In a batch the
longest radio wait counts for all. Each check after the script is limited to 25 s
(`CHECK_TIMEOUT`): checks of a device behind a link that was down came about every 2 min (the
connect to its neighbor had no limit yet), and berghof's batch missed the short while the
link was back.

`applyBatch` applies several devices together (`device::batch`; the device page's „Gemeinsam
anwenden“, `components::batch_apply`, with the other ends of its wireless links picked and the
devices of the sites to add, `DeviceAccess.batchCandidates`, each picked device's changes
shown before): one job per device (`job::start_all`, none if one is busy; cancelling one
cancels all; the page shows all jobs as equals, side by side where there is room,
`div.batch-jobs`), and one fallback for all, firing at one moment; each script brings its own
forward to the window at its end, which is about the same moment, and the batch waits no longer
than the earliest fallback a reached device reports, nor once one has fired. Each step runs on all
devices at once, the next only once all are through: planning (it also opens a second
connection, used up to the start, so no login falls between the steps), then fallback and
script on each (`write_script`, `ApplyWritten`: the script starts with `:delay 15s`), then
the start on all at once (`/execute` running the script in the background, then seen in
`/system/script/job`; `ApplyStarted`, its `error` `SCRIPT_NOT_STARTED` after 10 s; checked on
a CHR: the script goes on when the connection drops). The delay only lets every start get
through before the first changes cut a connection; a started script is never stopped (the
operator's call). Starting the scripts one after the other failed in the lab: the AP of the
60 GHz link cut the way to the station before its script could start, and the station,
reached over another way, came back unchanged. So the fallbacks are taken away only once every
device is reached, its script through (by its marks) and in line; otherwise none is (`BatchIncomplete` with the unreached and the
not applied, `FallbackLeftReason::NotApplied`/`BatchIncomplete`), and all load their config
from before together: the operator rolled a radio link back by hand when its far end couldn't
be reached after a single apply. A script that didn't start leaves its device not in line,
so that too sends all back. A problem before the start (planning, arming, writing) takes the
scripts and fallbacks away again, with nothing applied (`BatchStopped`). The steps are the
trait `batch::Applying`, so the rules are tested with stand-ins (`batch::test::run`).

A device that doesn't answer gets its initial setup over `/tool mac-telnet` from a neighbor that
does (`device::bootstrap`, on the device page): `plan` picks a cabled port with an IPv4 (on the
port or its bridge) whose other end is reachable. MAC addresses are not kept in NetBox: the
operator picks the physical device from what the neighbor's `/ip/neighbor` shows on that port
(`BootstrapPlan.discovered`) or types its MAC; don't make anything depend on NetBox MACs.
`set_up` drives the neighbor's console over SSH
(`device::terminal`, russh; login flags `+ct400w` so long echoes don't wrap) through the factory
prompts (license, default-config notice, new password - each answered once, some versions print
`new password>` twice), sets password, address, API and a firewall accept, and takes the port out
of the factory bridge in the background. The port is found by its factory name
(`default-name`): a device with an old config has its ports renamed (berghof-hansueli; the
CHR tests rename the target's port). Factory bridges answer MAC-telnet only on the bridge's
MAC, which is also the one they announce. `/tool mac-telnet` is tried with `interface=` the
neighbor's port (its name read over the API, `MacTelnetSetup.neighbor_port`) and without: a
CRS (klr-sw-1) only answered with it, CHR 7.24.5 only without; the variant that got a login
prompt stays for that MAC. Before the console session `check_neighbor` reads the neighbor's
`/ip ssh forwarding-enabled` and stops with `NeighborNoForwarding` unless it is `local`/`both`
(a disallowed forward stays silent), so such a neighbor leaves the device untouched. RouterOS
takes a forward whose connection doesn't come about all the same (CHR 7.24.5): only an address
on the cable without a device fails it at once, a closed port closes it at the login, a
firewall dropping or rejecting it and a route elsewhere leave the login without an answer. `connect` falls back to that bootstrap
address when the primary IP doesn't answer yet (the loopback, which only the first apply
brings), directly first (berghof-stall-haus' 10.198.1.4 was routed to), then through an SSH
tunnel to the neighbor
(`terminal::Tunnel`, `direct-tcpip`; `device::Path::Neighbor`): API, re-diff and SCP all go
from the neighbor's address on the cable, so the device needs no route back before its first
apply brings OSPF. It tries every neighbor of `bootstrap::plans` in turn, the management
proxy's device first: another neighbor may be cut off itself. Every generated config has `/ip
ssh forwarding-enabled=local` for that; without it RouterOS never answers the forward (no
refusal), hence the timeout in `Tunnel::open`. Each connection over a neighbor gets an SSH
session of its own (`Path::Neighbor` holds the plan, `Tunnel::into_stream`): of two API logins
at once over one session through RouterOS the second got no answer 4 times in 5 (lab-np2 to
lab-sxt-a, 7.23.8; link-5-station logged such logins in, the answer never came back), with a
session each 15 of 15 went through. Still, of five logins at once over berghof-burri three
never reached berghof-hansueli (7.21.5, its log): a login over a neighbor gets 10 s, up to three
times (`NEIGHBOR_LOGIN_TIMEOUT`), then `NeighborNoAnswer`; the SSH login to the neighbor isn't
in that time, RouterOS spaces many at once out (CHR: 6 s, 18 s from about seven on). The
connect to the neighbor's SSH is limited to
`OPEN_TIMEOUT`, else Linux waits about 2 min. `bootstrap::wait_for_api` limits each try, each with its own
session (the setup of link-5-ap hung there for good). A DHCP client holding the setup address (link-5-station's old config, served across the radio
link) is turned off by the setup, which `:put`s that into the console log: RouterOS refuses the
address statically next to the lease ("already have such address", CHR 7.24.5), and the
address check counts the lease, so the apply would fail on the address or remove the client and
the lease with it, cutting its own connection (`mac_telnet_setup_takes_the_address_from_a_dhcp_client`).
Don't go back to static return routes on the device: with
`redistribute=static` they go out as /32s that beat the neighbors' connected networks. When the
mac-telnet session ends early, the following lines land on the neighbor's own console (this
once left setup routes on lab-np1), so every setup command is wrapped to do nothing where the
global `netboxProvisionerNeighbor` is set, which `set_up` sets on the neighbor only. The
setup's job keeps the neighbor busy too (`job::start_through`): an apply or a reboot of it would
cut the session, so neither starts meanwhile (`BUSY`), and the neighbor's page lists the job
(`jobs(deviceId:)` with the device set up as its `deviceId`). Nothing
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
radios. Only after a factory login: a device logging in with its credentials (one that lost
its address after an update, the update fallback's rescue) keeps the rest of its config, its CAP
included, and its log is on the device page (`AccessibleDevice.log`, the newest 30 entries without the
logins, topic `account`: the provisioner's own drowned the rest).

Nothing waits on a device or the way to it without a limit: over a radio link a connection can
stall for good without ending (jobs hung at the setup, before the upload, after it). Opening a
connection, directly or through the management proxy, has 20 s (`device::OPEN_TIMEOUT`, each
try); an SSH key exchange and login 30 s (`terminal::SSH_LOGIN_TIMEOUT`, also the tunnel's);
an API login 30 s and every API command 120 s without a reply (mikrotik-api's `LOGIN_TIMEOUT`,
`REPLY_TIMEOUT`, `Error::ReplyTimeout`; a reply restarts it, so a monitor that keeps answering
goes on). A command that rightly stays silent longer says so with
`send_command_with_reply_timeout`: the device-mode countdown (`None`, its own deadline), the
partition copy (`COPY_TIMEOUT`) and mikrotik-model's `/system/script/run` of an apply
(`SCRIPT_RUN_TIMEOUT`, 10 min). Downloads from MikroTik have 20 s to connect and 60 s per read
(`update::http`). A new wait on a device needs one of these, or its own timeout.

Only `/system/identity` tells which device answers at an address. `AccessibleDevice::create_client`
reads it, and one named like another NetBox device is `WrongDevice` (an address taken twice, a
DHCP client left on), which `connect` treats as a failed way and tries the next. Any other
identity (a factory `MikroTik` after the initial setup, an old name) only shows: the device page
has `access.connected` (address, the neighbor the tunnel goes through, identity) and warns when
the identity isn't the NetBox name, so the operator sees what an apply would land on. Why a
device couldn't be reached is an error at `topology/deviceById/access` (`nullable`: a returned
error would null the whole `deviceById`), which the page shows in its "unreachable" warning
instead of the banner.

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
Each kind of job has a state machine in `device_state` (`docs/jobs.md`; an apply
alone or in a batch shares `ApplyState`, an update and a downgrade `UpdateState`, a reboot, a
repartition and a partition boot `RebootState`, a NetBox write and its undo `NetboxState`;
`JobPhase` over them comes from the `job_phase!` macro): the work reports
each state it enters (`Log::enter`), a transition the table doesn't allow is logged as
`unexpected state` and kept (`Log::unexpected`, asserted empty in the batch tests and the CHR
update, device mode and leftovers tests) while the job goes on unchanged. Report the state before the
step that has an effect, and keep the table in line when a flow changes. `Job.phase` gives the
state as a union with an object per kind (`JobPhaseObject`, `ApplyPhase { state }`, …); the job
view shows „Stand: …“ while it runs and „Endete in: …“ when it failed or was cancelled
(`components/job.rs` `phase_line`, names as in the concept, `i18n::job::phase`).
Only the states that go on after a restart (`Restart::Continue`: an apply's `ScriptRunning`
and `StoppingFallback`, an update's `Rebooting`, `NotUpdated` and `StoppingFallback` with its
fallback set) are saved, one file per device in `state_dir/jobs/<netbox-id>.json`
(`saved_job`: written to a temporary file and renamed, removed on leaving them; `state_dir`
defaults to `state` next to `config.yaml`, the chart mounts the volume `state` there). The
server holds `state_dir/lock`, a second one on the same directory doesn't start. Files read at
the start wait in `saved_job::waiting` until their job takes them up; one no job takes up (its
device gone from NetBox, unreadable) is shown on the site selection (`savedJobs`,
`components::saved_jobs`) until discarded (`discardSavedJob`). Times in them are the server's
clock, never an `Instant`. Without `saved_job::open` (tests, the CHR harness) nothing is
written. A job hands what it needs to its `Log` (`Log::keep`); `Log::enter` writes the file
in a state that goes on (with that state) and removes it in any other, the job's end too.
`graphql::resume_saved_jobs` (at the start, after the topology) starts a job per file of a
device NetBox has, beginning with `Resumed` and `Log::resume_in`: an apply
(`SavedApply`: credentials by name, `ScriptRun`, the held watchdogs) connects until its
device answers and goes on watching its script (`AccessibleDevice::watch_script`, the part of
`apply_planned` after the start), holding the watchdogs again (`RadioHold::resumed`). An
apply with ad hoc credentials saves nothing. An update with its fallback set hands its
`UpdateRun` to `keep` (`update::update_keeping`; `update::update` keeps nothing, the CHR
tests use it) before `Rebooting`, `StoppingFallback` and `NotUpdated`; resumed
(`update::resume`, `graphql::resume_update`) it waits for the reboot counted from entering
`Rebooting` and goes on as before. A resumed job may enter its saved state once more
(`Log::resume_in`). Once the servers stop (SIGTERM, SIGINT) `binary` calls `job::shutdown`:
a job with its file written is left as it is, every other one cancelled and given 20 s (the
chart's grace period is 60 s, actix's own shutdown up to 30 s of it); one ending meanwhile
in a state that goes on keeps its file.
A batch member's file names the whole batch (`SavedBatch`); the batch goes on together
(`batch::resume`, `Rejoined` members connecting once they answer, then `batch::watch`, the part
of `batch::run` after the scripts start) only with every member saved and in NetBox, else each
file waits with its reason (`HeldBack`) and the fallbacks take it all back. Held watchdogs are
announced again at the resumed job's start (`RadioHold::announce_saved`).
A job says where it is before anything slow: `graphql::connect_for_job` reports `Connecting`
and `Connected` (address, the neighbor's tunnel, the identity) for the device and each supplier,
the update its `UpdateTarget` and `WatchdogNotSet` for a watcher without one, and an upload
`PackageUploading` once the device takes the file (0 bytes) and at each quarter
(`terminal::upload` sends 64 KiB parts; one that doesn't go through within 60 s is
`UploadStalled`, and the SSH login, the SCP start and each answer before have 60 s each,
`UploadNoAnswer` with the step). A job without events was indistinguishable from a hung
one (the upload of 11 MB over berghof-burri's radio took 15 s, but looked stuck).
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
"Strom aus und ein" (`device::power_cycle`, `powerCycle`, asked to confirm) cold boots a device
fed over PoE: the RouterOS device whose `pse` port is cabled to it (`DeviceAccess.powerSupply`)
runs `/interface/ethernet/poe/power-cycle` on that port (found by its factory name) for 5 s. It
is a job of the supplier, so it runs next to the device's own job (a device mode change
waiting for its confirmation, which a warm reboot doesn't give), and it waits while the device
or one it powers updates (`PowerChain::power_cut`, a reboot lease of the supplier, so their
next update waits for it too). It keeps that lease until the device answers again
(`power_cycle::wait_for_fed`, `Connected`; `NOT_BACK_AFTER_REBOOT` after the reboot timeout):
the port is still off when the command returns, and the device boots after. Never started by
the provisioner itself. "Jetzt zurück" of the apply fallback waits for the chain like a reboot
(`leftovers::clear` takes a `PowerChain`).

Every job of a device writes an entry into the device's journal in NetBox (`backend::journal`,
`docs/protocol.md`), whenever `netbox_write_token` is set (not `allow_netbox_write`, that's the
helpers'): created at the start (`job::write_journal`, a task per job on the jobs' runtime),
written again at each `Log::enter` and at the end (a `watch` tick, so only the newest state goes
out when NetBox is slow), tag `netbox-provisioner`, what the provisioner reads back in the custom
field `provisioner_job` (JSON), the text in German from `i18n::journal`. Who started it comes
with the start (`journal::Started`: the login's name, or `Resumed`). A failed write never stops
the job (`JournalNotWritten`, once per run of failures); after the end only the newest 20 of the
provisioner's entries on the device stay. A site's job (`SiteDiff`) writes none. A resumed job writes on in its entry (`SavedJob::journal_entry`), every other entry still running is set to interrupted at the start (`journal::close_interrupted`). The device page's „Verlauf“ (`components::history`) reads them back from `provisioner_job` (`DeviceAccess.journal`, typed: `JournalOutcome`, `JournalLeft`) and words them like the journal. Reading `AccessibleDevice.leftovers` compares what is on the device with what the entries say lies there (`journal::observe`, Z31: each job entry's `left` and `touched`, the kinds it armed or cleared): a leftover no entry names, a fallback gone after a reboot (fired) or without one (removed) is an observation entry, itself a snapshot, so the next read writes none.

What a job didn't see to the end leaves on a device (cancelled, the provisioner restarted, the
device unreached) the device page shows under its head (`device::leftovers`,
`AccessibleDevice.leftovers`, `components::leftovers`; `null` while a job of the device runs):
the apply fallback's entry („Fallback stoppen“, „Jetzt zurück“: its `on-event` run with
`/execute`, then waiting for the device), the update fallback's, the watchdogs' safety net
(„Jetzt wiederherstellen“; while a running job of another device holds them suspended, e.g.
the update of the device they watch, they show apart as `heldBy` without a button and
`clearLeftover` refuses with `LEFTOVER_HELD`: restoring them could reboot this device while the
other is away), `.npk` files, and the apply script only while it runs (it stays
after every apply, the next replaces it). Each action is a `Cleanup` job (`clearLeftover`);
nothing is cleared by itself (CHR test `leftovers_are_found_and_cleared`). In
`/system/script/job` the API session is a job too: never remove those wholesale.

NetBox's interface `mtu` is the L3 MTU of an Ethernet port or `wlan` radio (`setup_mtu`, and the
wireless target); `l2mtu` is raised to carry it with a VLAN tag, other interface types refuse
one. Both ends of a cable need the same (`port_mtu`): RouterOS 7's OSPF stays in ExStart/Exchange
between different MTUs and has no `mtu-ignore`, so a link is cut until both ends have it. A
rollout therefore goes outward from the management proxy's device: `connect` reaches a device
cut off from OSPF through the neighbor already changed (see the bootstrap fallback below), and
its apply brings the link back. A new MTU
restarts a `wlan` radio; on a DFS weather radar channel (5600-5650 MHz) it listens for radar for
10 minutes (ETSI) before it sends again. The lab's links have none, as in production.
NetBox's interface `speed` (kbit/s) fixes an Ethernet port's speed without auto-negotiation
(`setup_speed`, `device/ros/speed.rs`); the RouterOS speed names the medium, which for a cage
comes from the cable's NetBox `type` (DAC, fiber, copper). Both RouterOS ends need the same, and
the board must advertise it there. The model keeps a device's `speed` where none is given
(`speed: k`): RouterOS refuses it empty and a negotiating port ignores it.
`DeviceCfg::generate_from` (the real path) calls the target's steps itself, not
`BaseDeviceDataTarget::generate_from` (which only tests use): a step added to one must be in the
other too. PoE out (`setup_poe`) was only in the latter, so the real path never set it before.

A DHCP server's static leases come from NetBox (`device::ros::dhcp::static_leases`): every
device without RouterOS with an interface whose primary MAC (`Interface.mac`, upper case) and
IPv4 are in the server's prefix, the device's name as comment, the rest at RouterOS' defaults
(`lease-time=0s`, `parent-queue=none`, the flags `no`). The provisioner owns the static
leases: one NetBox doesn't know is removed, a dynamic one is left (`dynamic` is ro, so the
model never reads it as an entry). The address is left out of the pool like any other NetBox
address there. Only `primary_mac_address` is fetched: the stored export lacks
`mac_addresses`. The site overview counts removed leases (`removes_leases`) like addresses.
An interface with `dhcp-client` and `nat` (the internet uplink) also gets an `/ipv6/dhcp-client`
asking for a prefix (`enable_dhcp_v6_client`: `request=address,prefix`, pool `pd-<interface>`,
no network takes from it yet, a default route as the IPv4 client); one per interface on
RouterOS, so one the device has there keeps its settings but those, and its pool's name
(`keep_dhcp_v6_clients`, on `current` after the renames, which it follows), and clients
elsewhere stay as they are.
A `dhcp-client` on an `ospf` interface takes no default route (`add-default-route=no`; RouterOS
then hides `default-route-distance` and can't clear it, so it gets RouterOS' default 1, which
mikrotik-model reads for the hidden field): the default comes over OSPF, and one from DHCP
would be redistributed although it only works from that device. OSPF originates a default only
on a device with the custom field `propagate_default_route` (`originate-default=if-installed`,
RouterOS' own default), every other one gets `never`: an apply of the lab's uplink router, whose
DHCP default from the production network was still installed, once announced it back into
production. CHR test `the_default_route_of_an_internet_uplink_goes_out_over_ospf`: a router
with `internet`, `nat`, `dhcp-client` and the field gets its default by DHCP through its own
perimeter firewall, and the router behind it learns it over OSPF. OSPF redistributes connected, static and BGP routes (`ospf_redistribute`): BGP is
configured by hand (klr-sw-3), and its routes reach the network only so; a default route never
goes out by `redistribute` (CHR 7.24.5), only by `originate-default`. A client network's (`dhcp`)
IPv6 /64 is advertised for SLAAC. IPv6 is on everywhere (`/ipv6/settings disable-ipv6=no`):
with it off the addresses are `invalid` and OSPFv3 never comes up, while the diff looks applied.

DNS servers come from the device's NetBox config context (`dns_servers`, in priority order):
client networks get them by DHCP (v4) and RDNSS (v6, only on devices with client networks),
every device asks them itself, and none resolves for others, but one with an own address in the
list (router-faesigrund-keller's loopbacks): it asks the others only and resolves. Without the
context, a router with a DHCP server answers DNS (its networks get its own address), and one
without a `dhcp-client` forwards to the loopbacks of the `nat` routers. The perimeter firewall
keeps the internet out of the resolver (the `close_dns` rules before it went with it).
`mu; ordered` makes a list field a `Box<[V]>` whose order counts (`dns-server`, `servers`).
An uplink tagged `nat` gets a masquerade rule. In the firewall menus (`/ip/firewall/filter`,
`nat`, `/ipv6/firewall/filter`, `address-list`) only the entries whose comment starts with
`netbox-provisioner:` are the provisioner's (`owned` in `mikrotik_model!`, Z1); every other
entry stays. Its rules come first, in the target's order (the filters are lists, the order of
the rules counts), before the others (`ordered`: a `move [find comment=…] destination=<first
other>`), but those whose comment goes on with `last ` after the prefix, which go after all
others (mikrotik-model `OWNED_LAST`): the drop ending a chain mustn't take what a hand-made rule
lets in. Each own comment is unique in its menu (`firewall_comment`), and has no `_`
(RouterOS's script escapes it). A rule without a comment that is an own one but for it is taken
over: the rules of the versions before had none.

The perimeter firewall (`device/ros/firewall.rs`, `docs/firewall.md`) guards every device with
an interface tagged `internet` or `nat` (`nat` needs `internet`, `NatWithoutInternet`): those
are the list `uplink`, and every rule is from `in-interface-list=uplink`, the rest of the
network routes without any. IPv4 and IPv6 alike: established/related in, invalid out, ICMP(v6),
the DHCPv6 client's answers (UDP 546 from `fe80::/10`), SSH through the address-list stages of
MikroTik's bruteforce guide (from the last stage back, so a connection takes one step; times
from the config context `ssh_stage_time`/`ssh_blacklist_time`, else 1 min and 10 min: the
guide's 10 days would lock an operator out that long), on `forward` IPv4's dstnat,
then the drops. `api_networks` come right after `invalid` (`allow_provisioner`, from
`generate_for` like `setup_services`, which tests call on `DeviceCfg` themselves): the
provisioner's SSH must never count in the stages (an update logs in and uploads several times).
A single IPv4 address in a rule is written without `/32` (`ros_address`): RouterOS reports it so
and the diff never got empty; an IPv6 one keeps its `/128`. CHR test
`perimeter_firewall_keeps_the_internet_out_but_the_provisioner`: the apply goes through the
firewall it sets up, from the uplink's side, and a new connection after it gets in and sees an
empty diff (the harness leaves out what the CHR's virtio ports take silently and keep: flow
control, `l2mtu`). An IPv6 address-list entry a rule adds shows up a moment later, so the test
waits for it.

NetBox services tagged `forward` (C2, `topology/forwards.rs`, `Topology::forwards`) open their
ports through it: an IPv4 address gets `dstnat … to-addresses=<it>` on its site's one router
with `nat` (the site of its prefix or the nearest above with one, else the service's device's;
the same port outside, Z3), an IPv6 one a `forward accept dst-address=<it>/128` before the drop
on every router with an uplink. `/ip/firewall/nat` is an ordered list like the filters (comments
`masquerade <if>`, `dstnat <addr> <proto>/<port>`). Findings `ForwardWithoutRouter` (no or
several `nat` routers), `ForwardPortTaken` (a port per router once, lower service id wins),
`ForwardIncomplete`. Older exports lack `service_list` (`example_topology` adds it empty).

`/ip/service` (`setup_services`, in `generate_for`) starts from the device's services, so what
an operator set stays, and turns telnet and ftp off (A2; packages go up by SCP over SSH). www
(WebFig over plain HTTP) stays on, turned on where it was off: in the field it is often the
only way in, Winbox isn't at hand; www-ssl would need a certificate on every device (A7).
With `api_networks` the plain API and www answer only those, written as `address`, which RouterOS
7.24 still takes for its `available-from`, unless the device reports `available-from` already
(an open service reports neither). `generate_live` reads the API's open connections (dynamic
`api` entries with `remote`) and leaves the API as it is, with `ApiClientsOutside`, where one
is outside the list: the provisioner comes from the management proxy's address towards the
device, over IPv6 where the device has one. RouterOS writes `remote` with the port and an IPv6
one without brackets (`2a02:610:7501:1000::201:0`), so the address is what is before the last
colon (`remote_address`).

A bridge snoops IGMP/MLD (`snoop_multicast`, without a querier): without it Android and
ChromeOS, which take IPv6 by SLAAC only, got no address in a WLAN behind srv-router-berg while a
PC on the LAN did, and turning it on at the router's bridge fixed it (2026-10-26). Only a bridge
without radio, VXLAN or CAPsMAN leaves it off, where a chip of its ports would offload it
without snooping but not with it (`L2Setup::snoops_multicast`).

A WLAN with a VLAN ends where NetBox puts its network (e.g. the controller, over the VLAN
group's VXLAN). A tagged bridge whose only way to other bridges is one VXLAN (a CAP's
`bridge-caps`; radios don't count) runs no STP: it closes no loop, and MSTP over the VXLAN,
a segment shared with every bridge of the group, blocked its port (ap-buero-1og, 7.23.7:
clients associated but got no DHCP). Without STP RouterOS reports none of the bridge's STP
settings but `forward-reserved-addresses=no` (7.24.5) and keeps them hidden; it rejects
clearing them (`forward-delay=""`, ap-klr-solar), so the target leaves them at RouterOS's
defaults, which mikrotik-model reads for a hidden one. With one VLAN in the group the CAP sets it: with `wifi-qcom` its radios get a
datapath with `vlan-id` into the bridge carrying that VLAN; `wifi-qcom-ac` radios can't tag
(MikroTik: "802.11ac chipsets do not support this type of VLAN tagging"; clients associate but
get no DHCP), so they are static access ports of that bridge instead
(`RadioPackage::tags_vlans`). With several (`WlanVlans::PerWlan`) the controller's configurations
carry each WLAN's `datapath.vlan-id` and the CAP's datapath only the bridge (MikroTik's CAPsMAN
VLAN example; no 802.11ax device in the lab to try it); a `wifi-qcom-ac` CAP is refused then,
it runs `wireless` instead with `wlan<N>` in NetBox (`hw_facts::runs_wireless`, the wAP ac).
The first wAP ac (`RBwAPG-5HacT2HnD`, mipsbe, one port) has no `wifi-qcom-ac` and only runs
`wireless`, like the hAP ac.
So does the cAP 2n (`RBcAP2n`, mipsbe, 2.4 GHz, one 10/100 port that advertises 1G; values as
bachman-wlan-dg reports them on 7.20.4), a CAP of the legacy CAPsMAN; its switch chip
(`switch1`) isn't in MikroTik's table and stays out of `SwitchChips`, which changes nothing for
one port beside a radio.
A CAP's radio refers to a local `/interface/wifi/configuration` (`manager=capsman`, the
datapath; `capsman` for the first datapath, `capsman-<bridge>` for others) and keeps its
default name (`wifi1`; L2 names it so too): CAPsMAN binds it through that configuration. A
bound radio is flagged dynamic and RouterOS refuses any change to it, also with the CAP off
(hAP ax², 7.23.8: `can not change dynamic interface`), or only bound (B): RouterOS takes a
change but reports and uses the controller's values (router-stall-burri, also a hAP ax² on 7.23.8:
`!datapath.vlan-id` stayed open with the controller's `.vlan-id=1`). So it stays as read
(`bound_radios`, from `InterfaceWifiState::dynamic` or `bound`), the configuration and datapath it uses stay, and a
different configuration than NetBox's is no warning: bound, it runs with the controller's values. What changes is the
configuration it uses (`datapath=`), a plain entry. A bound radio reports
`configuration=capsman` also where no such entry is (cAP ax, 7.23.7). It needs no seed in the
sort (`provided_by_routeros`): a kept reference isn't a dependency (`written_references`), and
the target adds the entry. As a seed it let unbound radios get `configuration=capsman` before
the add, which RouterOS refused (router-stall-burri).
A legacy radio (`wireless`) CAPsMAN manages refuses any change too (ap-klr-solar, 7.23.7:
`interface managed by CAPsMAN`), and the API has no flag for it: one the enabled CAP lists
(`WirelessDeviceDataCurrent::managed_radios`) keeps its name, which L2 uses for it
(`l2::KeptNames`, so bridge port and CAP match), and its settings
(`keep_managed_radios`); that they differ from NetBox is no warning, CAPsMAN configures them.
The CAP lists each radio by the name it gets (L2's, else the device's), never by swapping a default
name for a name afterwards: on bachmann-keller-ap (first wAP ac) `wlan1` is named `wlan2` and the
other way round, and such a swap dropped `wlan1` from the CAP (`swapped_radio_names_stay_in_the_cap`).
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
start while a supplier reboots (both under one lock). The leases are in-process only. They only hold back reboots the provisioner sends: a supplier with
`/system/watchdog watch-address` behind the updating device reboots by itself and cuts its
power mid-install (berghof-burri bricked burri-schopf that way; RouterBOOT then only asked for
Netinstall).

Every device gets RouterOS's logging (`device::ros::syslog`): its actions and rules, which it
refuses to remove (a target without them would remove them), with the `remote` action always in
CEF. With `device_syslog` set (`generate_for`) the action goes to that server and a rule per
topic sends to it (default `info`, `warning`, `error`, `critical`, as the devices had it, see
`examples/`); rules NetBox doesn't know of go. The action stays in CEF without a server too
(`remote=0.0.0.0`, no rule to it): RouterOS hides `syslog-time-format` and
`cef-event-delimiter` in the default format and refuses to clear them (`=""`, `!field`), so the
way back would fail. The hidden delimiter is RouterOS's `\r\n` once in CEF:
`syslog::after_switch_to_cef` makes the diff see that, so the target's `""` goes in the same
`set` (CHR 7.24.5: a second diff after either apply is empty).

Every device gets watchdogs (`docs/watchdog.md`, `device::ros::watchdog`): the
hardware one, a ping watchdog on its parent's primary IPv4 and, on a `pse` port powering a
RouterOS device, `power-cycle-ping` on that device. The parent comes from
`topology::management::ManagementTree`: roots are the `propagate_default_route` devices and
the management proxy's (`generate_for` adds it, `generate_from` knows only the first), two
RouterOS devices are neighbors where both have an `ospf` interface in the same IPv4 network
(a radio bridge's segment makes all four ends neighbors), the parent is the neighbor nearest
a root, a cabled one first (lab-sxt-b watched lab-np1 across its own radio before). The PoE
port pings the powered device's IPv4 at the cable (the setup gives it, the loopback comes with
the first apply). Only addresses that answer three pings from the device are set
(`DeviceCfg::generate_live`, in the plan, every check and `generateCfg`): a silent one keeps
what the device has and is the warning `WatchTargetSilent`, so a device being set up isn't
power cycled every 5 min and a check after the apply sees no diff from it. A device reaching its parent only through ports that power what they are cabled to
watches nothing: its reboot would cut that power (also the far router of a radio bridge it
powers). The ping watchdog reboots after one `ping-timeout` without answer, not several, and
10 min is RouterOS' maximum (CHR 7.24.5), so an update cuts it short: the watchers of a
device (`device::watchdog::watchers`: suppliers, `ManagementTree::depending_on`) are read for
the device page's warning (`DeviceAccess.watchdogs`: watch address, the PoE ports pinging the
device), and `updateRouteros(suspendWatchdogs: true)` sets them to `none` and
`power-cycle-ping-enabled=no` for the update and restores them after it whatever its outcome
(events `WatchdogSuspended`, `WatchdogRestored`, `WatchdogNotRestored`; a non-supplier that
can't be suspended is `WatchdogLeft` and the update goes on, a supplier stops it). A scheduler
entry on each, added before they go, restores them after `SAFETY_NET` (2 h) should the
provisioner not (CHR test `watchdog_is_restored_or_restores_itself`); a leftover entry is
where `read` finds them. An apply whose plan changes a radio (a DFS AP listens 10 min or longer,
as long as `ping-timeout`) suspends the watch address of the device and of those depending on
it, not the PoE pings (a port only pings what is on its cable), without asking
(`device::radio_hold`): its own makes the plan and the checks keep what the device has
(`PlannedApply::hold_watchdog`, `AccessibleDevice.watchdog_held`), and it is restored as NetBox
has it where that answers, else as it was. Restored once the fallback won't fire (back, or
nothing applied); while it may (not reached, cancelled after the script) they stay for the
scheduler entries (`WatchdogKept`). In a batch each radio-changing member holds its own and the
dependents outside the batch, each once; a member without a radio change depending on one
isn't held.

A `pse` port gets `poe-out=auto-on`, with a NetBox `poe_type` `passive-…` `forced-on`
(`setup_poe`): `auto-on` feeds only a load RouterOS detects, which a UISP Wave Nano isn't
(lab-np1 `e08`, 7.23.8: `waiting-for-load`; `forced-on` reports `powered-on` like `auto-on`, so
the PoE checks hold). A modeled port of a PoE board without `pse` gets `poe-out=off`, which cuts what it feeds and
which the script alone doesn't make obvious, so `generateCfg` returns `warnings` next to the
script (`device::ros::warning::ApplyWarning`, a union `i18n::device::warning` words; `PoeCut` from
`BaseDeviceDataTarget::poe_warnings`: on at the device, off in the target, and
powering something: `/interface/ethernet/poe/monitor` reports `poe-out-status=powered-on`, not
`waiting-for-load`; read only where a port would lose PoE, and when it can't be read every such
port warns), and the device page shows them above the changes. `CapOffOutsideWlanGroup`: a
board with radios in no Wireless LAN Group whose radios NetBox doesn't use serves no WLAN after
the apply (a CAP of `/interface/wifi` goes off, one of `/interface/wireless` is left as it is,
which is off after the initial setup); with `wireless` installed it warns only while that CAP
is off. It caught an AP missing from its group in NetBox.

mac-telnet for remote maintenance: the generated config has an interface list `maintenance`
(it was `mac-server`) and sets `/tool mac-server allowed-interface-list` and `/ip/neighbor
discovery-settings discover-interface-list` to it (`setup_maintenance_ports`, after
`setup_l2`, which names the ports). The list holds the ports cabled to a device with RouterOS
in NetBox, the radios of a wireless link, and every free Ethernet port: not in NetBox, or there
without cable, address, VLAN and bridge, so a notebook on a free port on site can get in. It
takes the port itself, which RouterOS honors for a bridge port too (CHR 7.24.5), so customer
ports in the same bridge stay out. Interface lists are modeled whole: an apply removes every
list NetBox doesn't give, the factory `LAN` included; `nat` uplinks go in `uplink`
(`add_to_uplinks`), which hand-made firewall rules use (router-faesigrund-keller's IPv6 filter
rejects what comes in there), so that list stays. That is what left lab devices with
`allowed-interface-list=*2000011` before, and `discover-interface-list=*2000011` (discovery
off) until the generator set it too.

The device mode (`/system/device-mode`, RouterOS 7.17+) locks features the provisioner needs
(`device::device_mode::wanted`: `scheduler` for the apply fallback, `socks` on the management
proxy's device, `partitions` where `/partitions` reports 128 MiB or more for the planned update
fallback, `docs/update-fallback.md`). A change needs a confirmation at the device
(button, or a cold boot) while `/system/device-mode/update` counts down; closing its connection
cancels it and a warm reboot doesn't confirm (CHR 7.24.5), so the `DeviceMode` job holds the
connection until the device goes down, sends only features that are off (an update changing
nothing counts down too), and is done when the device mode reads back with them on
(`device_mode_change_confirmed_by_a_cold_boot`, `RunningNode::power_cycle`). Without the
scheduler (`home`) RouterOS refuses a scheduler entry ("not allowed by device-mode",
berghof-hansueli 7.21.5): the apply then goes on without a fallback, a warning in the job
(`FallbackUnavailable`, `fallback::arm` reads the device mode first and saves no backup), with
nothing to stop or leave afterwards, in a batch too (`home_mode_has_no_fallback`).

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
sees the device. Then the OSPF neighbors it was adjacent with before have to be again within
3 min, else `OSPF_NEIGHBORS_MISSING`: `Full`, or `TwoWay` where DR and BDR are two others of the
segment (`update_fallback::adjacent`), since two DROthers stay there and a reboot elects anew
(berghof-stall-haus, 7.23.8: a v3 neighbor `Full` as DR before stayed `TwoWay`). The way back is the device page's "Von partN starten"
(`bootPartition`), never automatic. A device without two partitions updates as before and gets
an `UpdateWithoutFallback` event naming the neighbors it stays reachable through
(`update_fallback::WayBack`, `DeviceAccess.updateNeighbors`: those of `bootstrap::plans`, none
for the management proxy's device); without any, the update needs `withoutFallback: true`
(the device page's checkbox) and stops with `NO_UPDATE_FALLBACK` before downloading anything.
`downgradeRouteros` (`update::downgrade`, the device page's version field, prefilled with
`RouterOsVersions.previous`: the version before an update of this run, from the in-process
jobs) takes RouterOS 7 back to an older 7 (`NO_DOWNGRADE` otherwise): the packages it runs, of
that version, uploaded as for an update, then `/system/package/downgrade`, which reboots
(refused, the packages are removed again). It copies nothing to the other partition, which may
hold the version to go back to; without two partitions it needs the same way back as an
update. Watchdogs and OSPF neighbors as for an update, the firmware stays. Checked on lab-core (7.23.8 → 7.24.5 and back, both ways):
`copy-to` ends by itself after about 7 s. Booting `part1` brings back the old version with its
configuration and without the scheduler. The scheduler switching back by itself was checked on
lab-ap1 (provisioner stopped during the update's reboot: back on the old partition with its
configuration, the watchers' watchdogs restored by their safety net).

`local/` runs the lab: `netbox/` (NetBox + `seed-lab.py`), `keycloak/` (realm `lab` on
`localhost:8180`, admin/admin), `lab/make-config.sh` (the lab devices' password comes from the
ignored `.lab-netbox/device-password`, never put it in the repo) + `lab/run-provisioner.sh`
(builds frontend and binary, serves on :8082; the lab config lets the helpers write to the lab
NetBox with its admin token), `screenshots/` (Playwright, `run.sh STEP...`, with the browsers of
nixpkgs and the npm package of their version, installed unsaved when it differs from
`package.json`; a page reload asks
for the Keycloak login again, so navigate with `click:` instead of `goto:`, and `clicklast:` for
a card title whose text a link above has too). The harness's
`mac_telnet.rs` and `update.rs` cover setup and update against CHRs, reaching the new node
through a tunnel to the entry like the provisioner: `ensure_factory_image` gives
an untouched image of a `ChrRelease` (`CHR`, or `CHR_OLDER` to update from), `start_node` boots
it without waiting for the API, and `mac_of(node, slot)` is the MAC of a cabled NIC (slot 0 keeps
QEMU's, the golden image's dhcp-client is bound to it).

## Code conventions

- Tests of more than a few cases go in a module of their own: `<module>/test.rs`, or for a
  module with several, a directory `<module>/test/` (`device/ros/test/`, one file per topic),
  never `*_test.rs` files next to the code. Fixtures several modules' tests use belong in
  `topology::test` (`example_topology`, the stored export as a topology);
  `topology::builder::TopologyBuilder` isn't `cfg(test)`, the CHR tests build with it too.
- A RouterOS command no mikrotik-model type covers (actions, monitors, a menu read for one
  field) goes through `device::raw` (`command`, `rows`, `field`), which turns a `!trap` into
  `Error::Refused` naming the command; don't write another loop over `SimpleResult`.
- The backend sends data, the frontend words it: no sentence for the user is built in the
  backend (no German there at all; the journal takes its text from the crate `i18n`). What the UI shows is a typed GraphQL object or union with
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
- Every text the UI shows is in the crate `i18n` (German), a module (file) per page or
  component, a `const` or a `fn` taking the values; components write none inline, so a second
  language replaces that one crate. It takes plain values only (`&str`, numbers, its own enums;
  an error as `i18n::error::Problem`), so the backend can word with it too where it writes for
  people. What builds `Html` or words the frontend's own types (`findings`, `neighbors`,
  `target`, `interfaces`) stays in `frontend/src/i18n/`, which passes the crate's modules on as
  `crate::i18n::…`. Names RouterOS and NetBox use themselves (`RouterOS`,
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
- Push to `master` only when asked: a push to `master` publishes image and Helm chart as a
  snapshot (`ghcr.io/koa/netbox-provisioner:sha-<commit>`, chart in `oci://ghcr.io/koa/charts`
  as prerelease of `Chart.yaml`'s `version`), a tag `vX.Y.Z` as the release X.Y.Z, so this
  matters more here than in the other two repos. Snapshots are for a quick try: a daily
  cleanup (`cleanup.yaml`) deletes all but the newest.
- Verify with `cargo clippy` in addition to `cargo check`/`cargo test` — not every pre-existing
  warning needs fixing, but a change shouldn't leave new, avoidable ones behind.
