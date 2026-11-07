//! Scope: CHR tests of a radio link's shared OSPF segment; nothing else.
//!
//! A radio link's segment: the routers at both ends and the two bridge devices of the link
//! share one network, all four speaking OSPF on it. The bridge devices get priority 0 from the
//! generator, so neither is ever DR or BDR, also not after the link was cut and came back: the
//! routers are.
//!
//! The segment is a fifth CHR as a switch (a bridge over four cables): the harness's cables are
//! point to point, and QEMU's multicast sockets would need the host's real network.

use crate::chr_network::default_route::apply_directly;
use crate::chr_network::scenario::boot_scenario;
use crate::chr_network::wait::wait_until;
use backend::topology::builder::TopologyBuilder;
use backend::topology::{
    Cable, CablePort, Device, DeviceId, DeviceRole, Interface, IpAddressData, PhysicalPortId,
    TopologyHolder,
};
use mikrotik_api::prelude::MikrotikDevice;
use mikrotik_api::simple::SimpleResult;
use mikrotik_model::hwconfig::DeviceType;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio_stream::StreamExt;

struct Segment {
    builder: TopologyBuilder,
    switch: DeviceId,
    /// the near router, the two bridge devices, the far router
    speakers: [DeviceId; 4],
}

fn build() -> Segment {
    let mut builder = TopologyBuilder::default();
    let address = |builder: &mut TopologyBuilder, ip: &str| {
        let id = builder.ip_addresses.next_id();
        builder.ip_addresses.insert(
            id,
            IpAddressData {
                ip: ip.parse().unwrap(),
                interface: None,
                prefix: None,
            },
        );
        id
    };
    // first, so it listens on all its ports
    let switch = builder.devices.next_id();
    builder.devices.insert(
        switch,
        Device {
            name: "segment".into(),
            has_routeros: true,
            ..Default::default()
        },
    );
    let mut speakers = Vec::new();
    for (index, (name, bridge)) in [
        ("near", false),
        ("bridge-a", true),
        ("bridge-b", true),
        ("far", false),
    ]
    .into_iter()
    .enumerate()
    {
        let loopback = address(&mut builder, &format!("192.168.255.{}/32", 21 + index));
        let device = builder.devices.next_id();
        builder.devices.insert(
            device,
            Device {
                name: name.into(),
                primary_ip_v4: Some(loopback),
                loopback_ip: Some(loopback),
                has_routeros: true,
                role: bridge.then_some(DeviceRole::WlanBridge),
                ..Default::default()
            },
        );
        let ip = address(&mut builder, &format!("10.0.50.{}/29", 1 + index));
        let port = builder.interfaces.next_id();
        builder.interfaces.insert(
            port,
            Interface {
                name: "ether2".into(),
                device,
                external: Some(PhysicalPortId::Ethernet(2)),
                ips: Box::new([ip]),
                use_ospf: true,
                ..Default::default()
            },
        );
        let switch_port = builder.interfaces.next_id();
        let ether = 2 + index as u16;
        builder.interfaces.insert(
            switch_port,
            Interface {
                name: format!("ether{ether}").into_boxed_str(),
                device: switch,
                external: Some(PhysicalPortId::Ethernet(ether)),
                ..Default::default()
            },
        );
        let cable = builder.cables.next_id();
        builder.cables.insert(
            cable,
            Cable {
                port_a: Box::new([CablePort::Interface(switch_port)]),
                port_b: Box::new([CablePort::Interface(port)]),
                kind: None,
            },
        );
        speakers.push(device);
    }
    Segment {
        builder,
        switch,
        speakers: speakers.try_into().unwrap(),
    }
}

async fn run(
    device: &MikrotikDevice<SimpleResult>,
    command: &str,
    attributes: &[(&str, &str)],
) -> Vec<HashMap<Box<str>, Option<Box<str>>>> {
    let mut s = device
        .send_command(
            command.as_bytes(),
            |cmd| {
                attributes.iter().fold(cmd, |cmd, (key, value)| {
                    cmd.attribute(key.as_bytes(), value.as_bytes())
                })
            },
            (),
        )
        .await;
    let mut rows = Vec::new();
    while let Some(r) = s.next().await {
        match r {
            SimpleResult::Sentence(row) => rows.push(row),
            other => panic!("{command}: {other:?}"),
        }
    }
    rows
}

/// The state of the node's OSPFv2 interface on `ether2` (`dr`, `bdr`, `dr-other`, `waiting`, …),
/// and the DR it knows there.
async fn role(device: &MikrotikDevice<SimpleResult>) -> (String, String) {
    run(device, "/routing/ospf/interface/print", &[])
        .await
        .into_iter()
        .find(|row| {
            row.get("interface").cloned().flatten().as_deref() == Some("ether2")
                && row
                    .get("address")
                    .cloned()
                    .flatten()
                    .is_some_and(|a| a.starts_with("10.0.50."))
        })
        .map(|row| {
            let field = |name: &str| {
                row.get(name)
                    .cloned()
                    .flatten()
                    .unwrap_or_default()
                    .to_string()
            };
            (field("state"), field("dr"))
        })
        .unwrap_or_default()
}

#[tokio::test]
#[ignore]
async fn bridge_devices_are_never_dr_or_bdr_of_their_segment() {
    if crate::chr_network::qemu::nix_unavailable() {
        eprintln!("skipping: `nix` is not available in this environment");
        return;
    }
    let golden = crate::chr_network::golden_image::ensure_golden_image(
        &crate::cache_dir(),
        crate::TEST_PASSWORD,
    )
    .await
    .expect("golden image build failed");
    let Segment {
        builder,
        switch,
        speakers,
    } = build();
    let boards: HashMap<_, _> = [switch]
        .into_iter()
        .chain(speakers)
        .map(|device| (device, DeviceType::RB750Gr3))
        .collect();
    let work_dir = Path::new("/tmp").join("chr-network-ospf-segment-test");
    let nodes = boot_scenario(&golden, &builder, &boards, &work_dir, crate::TEST_PASSWORD)
        .await
        .expect("scenario boot failed");
    let holder: TopologyHolder = builder.into();

    // by hand: the switch bridges its four ports
    let switch_api = nodes[&switch]
        .connect_api(crate::TEST_PASSWORD)
        .await
        .expect("switch api");
    run(&switch_api, "/interface/bridge/add", &[("name", "segment")]).await;
    for ether in 2..=5 {
        run(
            &switch_api,
            "/interface/bridge/port/add",
            &[
                ("bridge", "segment"),
                ("interface", &format!("ether{ether}")),
            ],
        )
        .await;
    }
    for speaker in speakers {
        let access = holder.devices_by_id(speaker).await.unwrap();
        apply_directly(&nodes[&speaker], &access).await;
    }
    let mut apis = Vec::new();
    for speaker in speakers {
        apis.push(
            nodes[&speaker]
                .connect_api(crate::TEST_PASSWORD)
                .await
                .expect("api"),
        );
    }
    let [_, bridge_a, bridge_b, _] = &apis[..] else {
        unreachable!()
    };
    let roles = || async {
        let mut roles = Vec::new();
        for api in &apis {
            roles.push(role(api).await);
        }
        eprintln!("roles (near, bridge-a, bridge-b, far): {roles:?}");
        roles
    };
    let routers_elected = |roles: &[(String, String)]| {
        let elected = |state: &str| state == "dr" || state == "bdr";
        elected(&roles[0].0)
            && elected(&roles[3].0)
            && roles[1].0 == "dr-other"
            && roles[2].0 == "dr-other"
    };

    wait_until(
        || async { Ok(routers_elected(&roles().await)) },
        Duration::from_secs(180),
    )
    .await
    .expect("the routers are DR and BDR, the bridge devices neither");

    // the DR goes away: the other router takes over, and no bridge device steps up as BDR
    let dr = if roles().await[0].0 == "dr" { 0 } else { 3 };
    let other = 3 - dr;
    run(
        &apis[dr],
        "/interface/ethernet/disable",
        &[("numbers", "ether2")],
    )
    .await;
    wait_until(
        || async { Ok(roles().await[other].0 == "dr") },
        Duration::from_secs(120),
    )
    .await
    .expect("the other router becomes DR");
    // longer than the dead interval (40 s) and a wait after it
    tokio::time::sleep(Duration::from_secs(60)).await;
    for api in [bridge_a, bridge_b] {
        assert_eq!(
            role(api).await.0,
            "dr-other",
            "a bridge device is no BDR with one router left"
        );
    }

    // and back: the routers again, whichever is DR
    run(
        &apis[dr],
        "/interface/ethernet/enable",
        &[("numbers", "ether2")],
    )
    .await;
    wait_until(
        || async { Ok(routers_elected(&roles().await)) },
        Duration::from_secs(180),
    )
    .await
    .expect("the routers are DR and BDR again");
}
