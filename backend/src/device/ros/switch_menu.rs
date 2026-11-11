//! Scope: The VLAN menus of a CRS1xx/2xx switch (E17), from the VLANs `l2` gives it; not
//! the bridge (`l2::target`).
//!
//! A CRS1xx/2xx runs a VLAN-filtering bridge on the CPU, so its bridge stays without VLAN
//! filtering and the switch carries the VLANs (MikroTik's manual, „CRS1xx/2xx series switches
//! examples“, 7.25): an access port's untagged frames get its VLAN at the ingress
//! (`ingress-vlan-translation`, `customer-vid=0`), a trunk port and the CPU port send theirs
//! tagged (`egress-vlan-tag`), each VLAN reaches its members only (`switch/vlan`), and the
//! switch drops what comes in at a port outside its VLANs
//! (`drop-if-invalid-or-src-port-not-member-of-vlan-on-ports`). The CPU port is a member where
//! the device is in the VLAN, over a VLAN interface on the bridge.
use super::{SwitchMenuDataCurrent, SwitchMenuDataTarget, l2::SwitchVlan};
use mikrotik_model::{
    ascii::AsciiString,
    model::{
        InterfaceEthernetSwitchEgressVlanTagCfg, InterfaceEthernetSwitchIngressVlanTranslationCfg,
        InterfaceEthernetSwitchVlanByVlanId, InterfaceEthernetSwitchVlanCfg,
    },
};
use std::collections::BTreeSet;

/// The port of the switch towards the CPU, RouterOS's own.
pub(crate) const CPU_PORT: &[u8] = b"switch1-cpu";

impl SwitchMenuDataTarget {
    pub(in crate::device::ros) fn new(_model: &[u8]) -> Self {
        Self {
            switch: Default::default(),
            switch_vlan: Default::default(),
            egress_vlan_tag: Default::default(),
            ingress_vlan_translation: Default::default(),
        }
    }
    /// The menus for `vlans`, the switch itself as `current` has it but for the ports it
    /// filters: every member of a VLAN.
    pub(in crate::device::ros) fn generate_from(
        &mut self,
        vlans: &[SwitchVlan],
        current: &SwitchMenuDataCurrent,
    ) {
        let cpu = AsciiString::from(CPU_PORT);
        let mut filtered = BTreeSet::new();
        for vlan in vlans {
            let mut members = vlan
                .tagged
                .union(&vlan.untagged)
                .cloned()
                .collect::<BTreeSet<_>>();
            filtered.extend(members.iter().cloned());
            let mut tagged = vlan.tagged.clone();
            if vlan.cpu {
                members.insert(cpu.clone());
                tagged.insert(cpu.clone());
            }
            self.switch_vlan.insert(
                vlan.vlan_id,
                InterfaceEthernetSwitchVlanByVlanId(InterfaceEthernetSwitchVlanCfg {
                    vlan_id: vlan.vlan_id,
                    ports: members,
                    ..Default::default()
                }),
            );
            if !tagged.is_empty() {
                self.egress_vlan_tag.insert(
                    (vlan.vlan_id,),
                    InterfaceEthernetSwitchEgressVlanTagCfg {
                        vlan_id: vlan.vlan_id,
                        tagged_ports: tagged,
                        ..Default::default()
                    },
                );
            }
            if !vlan.untagged.is_empty() {
                let translation = InterfaceEthernetSwitchIngressVlanTranslationCfg {
                    ports: vlan.untagged.clone(),
                    // untagged frames
                    customer_vid: Some(0),
                    new_customer_vid: Some(vlan.vlan_id),
                    sa_learning: true,
                    ..Default::default()
                };
                self.ingress_vlan_translation.insert(
                    (translation.ports.clone(), translation.customer_vid),
                    translation,
                );
            }
        }
        self.switch = current.switch.clone();
        self.switch
            .drop_if_invalid_or_src_port_not_member_of_vlan_on_ports = Some(filtered);
    }
}
