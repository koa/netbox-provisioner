//! Scope: What the devices see at their ports against NetBox's cables, for a site or a device; not
//! asking for it (`site::neighbor_check`).
//!
//! What the devices see at their ports against NetBox's cables, worded with links into NetBox:
//! for the site's neighbor check (`site::neighbor_check`) and the live checks of a device's
//! interfaces (`device::interfaces`).
use crate::{
    components::findings::{netbox_link, severity_label},
    graphql::authenticated::{
        neighbor_findings::{
            NeighborFinding as Finding, NeighborFindingKind as Kind, SeenNeighbor as Seen,
        },
        types::{InterfaceRef as Interface, PortRef as Port},
    },
    i18n::neighbors as t,
};
use yew::{Html, html};

/// A neighbor finding in words.
pub fn describe(netbox: &str, kind: &Kind) -> Html {
    Links { netbox }.describe(kind)
}

/// Neighbor findings as a list, the worst first (the interface view's live checks); without
/// taking a cable over, the row's actions do that.
pub fn neighbor_finding_list(netbox: &str, findings: &[Finding]) -> Html {
    let links = Links { netbox };
    html! {
        <ul class="finding-list">
            { for findings.iter().map(|finding| html! {
                <li>
                    {severity_label(finding.severity)}
                    <span>{links.describe(&finding.kind)}</span>
                </li>
            }) }
        </ul>
    }
}

/// Who the device sees, linked into NetBox.
pub fn seen_list(netbox: &str, seen: &[Seen]) -> Html {
    Links { netbox }.seen(seen)
}

struct Links<'a> {
    netbox: &'a str,
}

impl Links<'_> {
    fn interface(&self, interface: &Interface) -> Html {
        netbox_link(
            self.netbox,
            &format!("dcim/interfaces/{}/", interface.id),
            &format!("{} {}", interface.device.name, interface.name),
        )
    }
    /// the port as the device names it, linked to its NetBox interface where there is one
    fn port(&self, port: &Port) -> Html {
        match &port.interface {
            Some(interface) => netbox_link(
                self.netbox,
                &format!("dcim/interfaces/{}/", interface.id),
                &port.name,
            ),
            None => html!({ port.name.clone() }),
        }
    }
    fn seen_one(&self, seen: &Seen) -> Html {
        match (&seen.interface, &seen.device) {
            (Some(interface), _) => netbox_link(
                self.netbox,
                &format!("dcim/interfaces/{}/", interface.id),
                &format!("{} {}", seen.identity, seen.interface_name),
            ),
            (None, Some(device)) => netbox_link(
                self.netbox,
                &format!("dcim/devices/{}/", device.id),
                &format!("{} {}", seen.identity, seen.interface_name),
            ),
            (None, None) => {
                html!({ t::unknown(&seen.identity, &seen.interface_name, &seen.board) })
            }
        }
    }
    fn seen(&self, seen: &[Seen]) -> Html {
        t::seen(seen.iter().map(|s| self.seen_one(s)).collect())
    }

    fn describe(&self, kind: &Kind) -> Html {
        match kind {
            Kind::CableMissing(f) => t::cable_missing(self.port(&f.port), self.seen(&f.seen)),
            Kind::CableElsewhere(f) => t::cable_elsewhere(
                self.port(&f.port),
                self.interface(&f.far_end),
                self.seen(&f.seen),
            ),
            Kind::OtherFarPort(f) => t::other_far_port(
                self.port(&f.port),
                self.interface(&f.far_end),
                self.seen_one(&f.seen),
            ),
            Kind::NeighborNotSeen(f) => t::not_seen(self.port(&f.port), self.interface(&f.far_end)),
            Kind::NeighborsNotInNetbox(f) => {
                t::not_in_netbox(self.port(&f.port), self.seen(&f.seen))
            }
            Kind::DiscoveryOff(f) => html!({ t::discovery_off(&f.list) }),
            Kind::Unknown => html!({ crate::i18n::findings::UNKNOWN }),
        }
    }
}
