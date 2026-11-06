//! What NetBox lacks or what doesn't fit together, for a site or the whole network
//! (`TopologyHolder.findings`), every object a link to it in NetBox.
use crate::{
    error::FrontendError,
    graphql::{
        authenticated::{
            findings::{
                Finding, FindingKind as Kind, Findings, FindingsVariables, PrefixPool,
                RadioPathPart,
            },
            types::{
                AddressRef, DeviceRef, InterfaceRef, IpVersion, ServiceRef, Severity, VlanRef,
            },
        },
        live::{Subscription, watch_data},
    },
    i18n::findings as t,
};
use cynic::SubscriptionBuilder;
use patternfly_yew::prelude::{Card, CardBody, CardHeader, CardTitle, Color, Label, Spinner};
use yew::{Component, Context, Html, Properties, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct FindingsProps {
    /// the site whose devices' findings are shown; without, those about the whole network,
    /// and nothing when there are none
    pub site: Option<u32>,
}

pub struct FindingsList {
    data: Option<Result<(String, Vec<Finding>), FrontendError>>,
    /// shown again when NetBox changed
    _subscription: Subscription,
}

pub enum FindingsMsg {
    Data(Result<(String, Vec<Finding>), FrontendError>),
}

impl Component for FindingsList {
    type Message = FindingsMsg;
    type Properties = FindingsProps;

    fn create(ctx: &Context<Self>) -> Self {
        let scope = ctx.link().clone();
        let variables = FindingsVariables {
            site: ctx.props().site.map(|site| site as i32),
            device: None,
            after: None,
        };
        let subscription = watch_data(ctx.link(), Findings::build(variables), move |data| {
            let data = data.map(|data| (data.topology.netbox_url, data.topology.findings));
            scope.send_message(FindingsMsg::Data(data));
        });
        Self {
            data: None,
            _subscription: subscription,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            FindingsMsg::Data(data) => self.data = Some(data),
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let site = ctx.props().site;
        let body = match &self.data {
            None if site.is_none() => return Html::default(),
            None => html!(<Spinner/>),
            Some(Err(error)) => error.to_html(),
            Some(Ok((_, findings))) if findings.is_empty() => match site {
                Some(_) => html!(<p>{t::NONE}</p>),
                None => return Html::default(),
            },
            Some(Ok((netbox, findings))) => finding_list(netbox, findings),
        };
        let title = if site.is_some() {
            t::TITLE
        } else {
            t::NETWORK_TITLE
        };
        html! {
            <Card class="findings">
                <CardHeader><CardTitle>{title}</CardTitle></CardHeader>
                <CardBody>{body}</CardBody>
            </Card>
        }
    }
}

/// The findings, each object a link into NetBox at `netbox` (ending in `/`).
pub fn finding_list(netbox: &str, findings: &[Finding]) -> Html {
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

/// How bad a finding is, as a colored label.
pub fn severity_label(severity: Severity) -> Html {
    let (label, color) = match severity {
        Severity::Error => (t::ERROR, Color::Red),
        Severity::Warning => (t::WARNING, Color::Orange),
        Severity::Hint | Severity::Other => (t::HINT, Color::Blue),
    };
    html!(<Label compact=true {color} label={label}/>)
}

/// A link to `path` in NetBox' web UI at `netbox` (ending in `/`).
pub fn netbox_link(netbox: &str, path: &str, text: &str) -> Html {
    let href = format!("{netbox}{path}");
    html!(<a {href} target="_blank" rel="noopener">{text.to_string()}</a>)
}

/// Links to the objects of a finding in NetBox' web UI.
struct Links<'a> {
    /// ends in `/`
    netbox: &'a str,
}

impl Links<'_> {
    fn link(&self, path: &str, text: &str) -> Html {
        netbox_link(self.netbox, path, text)
    }
    fn device(&self, device: &DeviceRef) -> Html {
        self.link(&format!("dcim/devices/{}/", device.id), &device.name)
    }
    fn interface(&self, interface: &InterfaceRef) -> Html {
        self.link(
            &format!("dcim/interfaces/{}/", interface.id),
            &format!("{} {}", interface.device.name, interface.name),
        )
    }
    fn address(&self, address: &AddressRef) -> Html {
        self.link(
            &format!("ipam/ip-addresses/{}/", address.id),
            &address.address,
        )
    }
    fn service(&self, service: &ServiceRef) -> Html {
        self.link(&format!("ipam/services/{}/", service.id), &service.name)
    }
    fn vlan(&self, vlan: &VlanRef) -> Html {
        self.link(
            &format!("ipam/vlans/{}/", vlan.id),
            &t::vlan_name(vlan.vid, &vlan.name),
        )
    }
    fn interfaces(&self, interfaces: &[InterfaceRef]) -> Html {
        t::list(interfaces.iter().map(|i| self.interface(i)).collect())
    }
    fn pool(&self, pool: &PrefixPool) -> Html {
        let tag = t::pool(matches!(pool, PrefixPool::Loopback));
        self.link(&format!("ipam/prefixes/?tag={tag}"), &format!("„{tag}“"))
    }

    fn describe(&self, kind: &Kind) -> Html {
        let version = |v: &IpVersion| t::version(matches!(v, IpVersion::V6));
        match kind {
            Kind::PoolMissing(f) => t::pool_missing(self.pool(&f.pool), version(&f.version)),
            Kind::PoolNearlyFull(f) => {
                t::pool_nearly_full(self.pool(&f.pool), version(&f.version), f.free.0, f.size.0)
            }
            Kind::NoLoopback(f) => t::no_loopback(self.device(&f.device)),
            Kind::NoPrimaryIpv4(f) => t::no_primary_ipv4(self.device(&f.device)),
            Kind::NoCountry(f) => t::no_country(
                self.device(&f.device),
                f.site
                    .as_ref()
                    .map(|site| self.link(&format!("dcim/sites/{}/", site.id), &site.name)),
                f.region.as_deref(),
            ),
            Kind::PrimaryIpv4NotOnLoopback(f) => t::primary_not_on_loopback(
                self.device(&f.device),
                self.address(&f.address),
                f.interface.as_ref().map(|i| self.interface(i)),
            ),
            Kind::AddressWithoutPrefix(f) => t::address_without_prefix(
                self.interface(&f.interface),
                self.address(&f.address),
                f.dhcp,
            ),
            Kind::DhcpPoolEmpty(f) => t::dhcp_pool_empty(
                self.interface(&f.interface),
                self.address(&f.address),
                f.prefix.as_ref().map(|prefix| {
                    self.link(&format!("ipam/prefixes/{}/", prefix.id), &prefix.prefix)
                }),
            ),
            Kind::PortLikeName(f) => t::port_like_name(self.interface(&f.interface)),
            Kind::RadioWithoutRole(f) => t::radio_without_role(self.interface(&f.interface)),
            Kind::NatWithoutInternet(f) => t::nat_without_internet(self.interface(&f.interface)),
            Kind::LoopbackWithoutOspf(f) => {
                t::loopback_without_ospf(self.device(&f.device), self.address(&f.address))
            }
            Kind::VlanWithoutGroup(f) => t::vlan_without_group(
                self.interface(&f.interface),
                self.link(&format!("ipam/vlans/{}/", f.vlan), &t::vlan(f.vlan)),
            ),
            Kind::MtuMismatch(f) => t::mtu_mismatch(
                self.interface(&f.interface),
                f.mtu,
                self.interface(&f.far_end),
                f.far_mtu,
            ),
            Kind::OspfOnOneEnd(f) => {
                t::ospf_on_one_end(self.interface(&f.interface), self.interface(&f.far_end))
            }
            Kind::NoCommonSubnet(f) => {
                t::no_common_subnet(self.interface(&f.interface), self.interface(&f.far_end))
            }
            Kind::AddressWithoutOspf(f) => {
                t::address_without_ospf(self.interface(&f.interface), self.interface(&f.far_end))
            }
            Kind::WlanWithoutVlan(f) => t::wlan_without_vlan(
                self.link(&format!("wireless/wireless-lans/{}/", f.id), &f.ssid),
                self.link(
                    &format!("wireless/wireless-lan-groups/{}/", f.group.id),
                    &f.group.name,
                ),
            ),
            Kind::WlanGroupWithoutController(f) => t::wlan_group_without_controller(self.link(
                &format!("wireless/wireless-lan-groups/{}/", f.group.id),
                &f.group.name,
            )),
            Kind::VlanWithSeveralGateways(f) => {
                t::vlan_with_several_gateways(self.vlan(&f.vlan), self.interfaces(&f.interfaces))
            }
            Kind::VlanWithoutGateway(f) => t::vlan_without_gateway(
                self.vlan(&f.vlan),
                (!f.interfaces.is_empty()).then(|| self.interfaces(&f.interfaces)),
            ),
            Kind::TaggedVlanOnPort(f) => t::tagged_vlan_on_port(
                self.interface(&f.interface),
                t::list(f.vlans.iter().map(|v| self.vlan(v)).collect()),
            ),
            Kind::RadioPathIncomplete(f) => t::radio_path_incomplete(
                self.interface(&f.radio),
                f.missing
                    .iter()
                    .filter_map(|part| match part {
                        RadioPathPart::RouterCable => Some(t::RADIO_ROUTER_CABLE),
                        RadioPathPart::WirelessLink => Some(t::RADIO_WIRELESS_LINK),
                        RadioPathPart::Ssid => Some(t::RADIO_SSID),
                        RadioPathPart::PeerRouterCable => Some(t::RADIO_PEER_ROUTER_CABLE),
                        RadioPathPart::Other => None,
                    })
                    .collect(),
            ),
            Kind::LabelNotFarEnd(f) => t::label_not_far_end(
                self.interface(&f.interface),
                f.label.as_deref(),
                self.device(&f.far_end.device),
            ),
            Kind::VlanWithoutPort(f) => {
                t::vlan_without_port(self.interface(&f.interface), self.vlan(&f.vlan))
            }
            Kind::ForwardWithoutRouter(f) => t::forward_without_router(
                self.service(&f.service),
                self.address(&f.address),
                self.link(&format!("dcim/sites/{}/", f.site.id), &f.site.name),
            ),
            Kind::ForwardWithoutSite(f) => {
                t::forward_without_site(self.service(&f.service), self.address(&f.address))
            }
            Kind::ForwardPortTaken(f) => t::forward_port_taken(
                self.service(&f.service),
                &f.port,
                self.device(&f.router),
                self.service(&f.by),
            ),
            Kind::ForwardIncomplete(f) => t::forward_incomplete(self.service(&f.service)),
            Kind::Unknown => t::UNKNOWN.into(),
        }
    }
}

/// The text for a card of a list: how many findings, how many errors.
pub fn count(severities: impl IntoIterator<Item = Severity>) -> Option<String> {
    let (mut all, mut error_count) = (0, 0);
    for severity in severities {
        all += 1;
        error_count += usize::from(severity == Severity::Error);
    }
    t::count(all, error_count)
}
