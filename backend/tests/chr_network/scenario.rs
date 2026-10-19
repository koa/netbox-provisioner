//! Derives VM wiring directly from a [`TopologyBuilder`] - the same type
//! production code and existing fixtures already use to describe a network
//! - instead of a second, parallel scenario-description DSL.

use crate::chr_network::golden_image::GoldenImage;
use crate::chr_network::qemu::free_tcp_port;
use crate::chr_network::vm::{PortWiring, RunningNode, boot_node};
use backend::topology::builder::TopologyBuilder;
use backend::topology::{CablePort, DeviceId, PhysicalPortId};
use mikrotik_model::hwconfig::DeviceType;
use std::collections::HashMap;
use std::path::Path;

/// Boots one disposable node per entry in `boards`, wired up per the cables
/// in `builder` that run directly between two of those devices (plain
/// interface-to-interface cables only - patch-panel chains through
/// `FrontPort`/`RearPort` aren't resolved here, see the project plan).
///
/// Slot 0 (`ether1`) on every node is always reserved for the
/// host-reachable management NIC (see `vm::boot_node`); scenario topologies
/// must not cable anything to it.
pub async fn boot_scenario(
    golden: &GoldenImage,
    builder: &TopologyBuilder,
    boards: &HashMap<DeviceId, DeviceType>,
    work_dir: &Path,
    password: &str,
) -> anyhow::Result<HashMap<DeviceId, RunningNode>> {
    let mut wirings: HashMap<DeviceId, Vec<PortWiring>> = boards
        .iter()
        .map(|(id, board)| {
            let nic_count = board.build_ethernet_ports().len();
            (*id, vec![PortWiring::Idle; nic_count.saturating_sub(1)])
        })
        .collect();

    // Deterministic boot order so "smaller DeviceId listens, larger
    // connects" below is always correct: every Listen side is fully booted
    // (not just spawned - `boot_node` waits for API reachability) before
    // any Connect side referencing it ever starts.
    let mut boot_order: Vec<DeviceId> = boards.keys().copied().collect();
    boot_order.sort_by_key(|id| id.0);

    for cable in builder.cables.values() {
        let (Some(a), Some(b)) = (
            ethernet_endpoint(builder, &cable.port_a),
            ethernet_endpoint(builder, &cable.port_b),
        ) else {
            continue; // not a plain interface-to-interface ethernet cable - out of scope here
        };
        if !boards.contains_key(&a.device) || !boards.contains_key(&b.device) {
            continue; // cable leaves this scenario's node set
        }
        anyhow::ensure!(
            a.slot > 1 && b.slot > 1,
            "cable uses ether1, which is reserved for the management NIC on every node"
        );
        let port = free_tcp_port()?;
        let (listener, connector) = if a.device.0 < b.device.0 {
            (a, b)
        } else {
            (b, a)
        };
        set_slot(
            &mut wirings,
            listener.device,
            listener.slot,
            PortWiring::Listen(port),
        );
        set_slot(
            &mut wirings,
            connector.device,
            connector.slot,
            PortWiring::Connect(port),
        );
    }

    let mut nodes = HashMap::new();
    for device_id in boot_order {
        let wiring = wirings.remove(&device_id).unwrap_or_default();
        let name = format!("device-{}", device_id.0);
        let node = boot_node(golden, work_dir, &name, &wiring, password).await?;
        nodes.insert(device_id, node);
    }
    Ok(nodes)
}

/// `ether_number` is the 1-based ether port number (`ether2` is `2`).
/// `vm::boot_node`'s `wiring[i]` describes NIC slot `i + 1`, and NIC slot 0
/// is `ether1` (reserved for management) - so `etherN` lives at NIC slot
/// `N - 1`, i.e. `wiring[N - 2]`.
fn set_slot(
    wirings: &mut HashMap<DeviceId, Vec<PortWiring>>,
    device: DeviceId,
    ether_number: usize,
    value: PortWiring,
) {
    if let Some(wiring) = wirings.get_mut(&device) {
        wiring[ether_number - 2] = value;
    }
}

struct Endpoint {
    device: DeviceId,
    /// 1-based ether number (so `ether2` is `2`), not the 0-based NIC slot.
    slot: usize,
}

fn ethernet_endpoint(builder: &TopologyBuilder, ports: &[CablePort]) -> Option<Endpoint> {
    let [CablePort::Interface(if_id)] = ports else {
        return None; // not a single, plain interface termination
    };
    let interface = builder.interfaces.get(if_id)?;
    let PhysicalPortId::Ethernet(n) = interface.external? else {
        return None; // combo/sfp/wifi - out of scope here, see the project plan
    };
    Some(Endpoint {
        device: interface.device,
        slot: n as usize,
    })
}
