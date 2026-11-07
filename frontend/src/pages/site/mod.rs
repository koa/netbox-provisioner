//! Scope: The page of a site with its devices; not its cards (`components::site`).
use crate::{
    components::{
        findings::{self, FindingsList},
        site::device_entry_card::{DeviceEntryCard, DeviceRow},
        site::neighbor_check::NeighborCheck,
        site::site_diff::SiteDiff,
    },
    error::FrontendError,
    graphql::{
        authenticated::list_devices::{SiteDevices, SiteDevicesVariables},
        live::{Subscription, watch_data},
    },
};
use cynic::SubscriptionBuilder;
use patternfly_yew::prelude::Spinner;
use std::{net::IpAddr, str::FromStr};
use yew::{Component, Context, Html, Properties, html};
pub mod device;
pub mod map;
#[derive(Debug, Clone, PartialEq, Properties)]
pub struct DevicesProps {
    pub site: u32,
}
/// The devices of a site
pub struct Devices {
    state: DeviceState,
    error_state: Option<FrontendError>,
    /// shown again when NetBox changed
    _subscription: Subscription,
}
enum DeviceState {
    Loading,
    Data(Box<[DeviceRow]>),
}
#[derive(Debug)]
pub enum DevicesMsg {
    Data(Box<[DeviceRow]>),
    Error(FrontendError),
}
impl Component for Devices {
    type Message = DevicesMsg;
    type Properties = DevicesProps;

    fn create(ctx: &Context<Self>) -> Self {
        let scope = ctx.link().clone();
        let site = ctx.props().site;
        let devices = SiteDevices::build(SiteDevicesVariables { site: site as i32 });
        let subscription = watch_data(ctx.link(), devices, move |data| {
            scope.send_message(match data {
                Ok(data) => DevicesMsg::Data(
                    data.topology
                        .site_by_id
                        .map(|site| site.devices)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|device| DeviceRow {
                            site,
                            id: device.id as u32,
                            name: device.name.into_boxed_str(),
                            address: device
                                .management_address
                                .and_then(|ip| ip.address)
                                .and_then(|a| IpAddr::from_str(a.ip.as_str()).ok()),
                            serial: device.serial.map(|s| s.into_boxed_str()),
                            findings: findings::count(device.findings.iter().map(|f| f.severity)),
                        })
                        .collect(),
                ),
                Err(error) => DevicesMsg::Error(error),
            })
        });
        Self {
            state: DeviceState::Loading,
            error_state: None,
            _subscription: subscription,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            DevicesMsg::Data(data) => {
                self.state = DeviceState::Data(data);
                self.error_state = None;
                true
            }
            DevicesMsg::Error(error) => {
                self.error_state = Some(error);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let site = ctx.props().site;
        let check = match &self.state {
            DeviceState::Data(rows) if !rows.is_empty() => {
                let devices = rows
                    .iter()
                    .map(|row| (row.id, row.name.clone()))
                    .collect::<Vec<_>>();
                html!(<NeighborCheck {devices}/>)
            }
            _ => Html::default(),
        };
        let error_msg = self.error_state.as_ref().map(FrontendError::to_html);
        let data = match &self.state {
            DeviceState::Loading => {
                html! {<Spinner/>}
            }
            DeviceState::Data(rows) => {
                let cards = rows.iter().map(|row| {
                    html! {
                        <DeviceEntryCard device={row.clone()}/>
                    }
                });
                html! {
                    for card in cards { {card} }
                }
            }
        };
        html! {
            <>
                <FindingsList site={Some(site)}/>
                <SiteDiff {site}/>
                {check}
                <div class="device-list">
                    {error_msg}
                    {data}
                </div>
            </>
        }
    }
}
