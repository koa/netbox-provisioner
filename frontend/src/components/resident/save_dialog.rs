//! Scope: The preview of a resident's drafts over the page: their plan in words, what the
//! devices notice, saving them together and how saving goes; not the drafts themselves
//! (`drafts`, `components::resident`).
use super::drafts::{Drafts, LinkDraft, PortKey};
use crate::{
    error::{FrontendError, ServerProblem},
    graphql::{
        request_resident,
        resident::{
            change::{
                ChangePlanQuery, ChangePlanVariables, InterfaceRef, ResidentPlan, SaveChange,
                SaveChangeVariables, SaveState, SavedChange, SavedChangeQuery,
                SavedChangeVariables,
            },
            residents::Resident,
        },
    },
};
use cynic::{MutationBuilder, QueryBuilder};
use gloo::timers::future::TimeoutFuture;
use i18n::resident::{self as t, PortAt};
use patternfly_yew::prelude::{
    Alert, AlertType, Bullseye, Button, ButtonVariant, Modal, ModalVariant,
};
use yew::{Callback, Component, Context, Html, Properties, html, platform::spawn_local};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct SaveDialogProps {
    /// the resident it saves for (a resident's own, or the one of the support view)
    pub resident: Resident,
    pub drafts: Drafts,
    /// back to the page, the drafts kept unless saved
    pub on_close: Callback<()>,
    /// the drafts are in NetBox: the page drops them and loads the resident again
    pub on_saved: Callback<()>,
}

/// Where the preview stands: from asking the plan over showing it to saving.
pub enum SaveDialog {
    Planning,
    Plan(ResidentPlan),
    Saving(Option<SavedChange>),
    Failed(FrontendError),
}

pub enum SaveDialogMsg {
    Plan(Result<ResidentPlan, FrontendError>),
    Save,
    Saved(Result<SavedChange, FrontendError>),
}

impl Component for SaveDialog {
    type Message = SaveDialogMsg;
    type Properties = SaveDialogProps;

    fn create(ctx: &Context<Self>) -> Self {
        let scope = ctx.link().clone();
        let variables = ChangePlanVariables {
            resident: ctx.props().resident.id,
            changes: ctx.props().drafts.changes(),
        };
        spawn_local(async move {
            let plan = request_resident(scope.clone(), ChangePlanQuery::build(variables))
                .await
                .map(|data| data.change_plan);
            scope.send_message(SaveDialogMsg::Plan(plan));
        });
        SaveDialog::Planning
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let state = std::mem::replace(self, SaveDialog::Planning);
        *self = match (state, msg) {
            (_, SaveDialogMsg::Plan(Ok(plan))) => SaveDialog::Plan(plan),
            (SaveDialog::Plan(plan), SaveDialogMsg::Save) => {
                save(ctx, plan.fingerprint);
                SaveDialog::Saving(None)
            }
            (_, SaveDialogMsg::Saved(Ok(saved))) => {
                if saved.state == SaveState::Saved {
                    ctx.props().on_saved.emit(());
                }
                SaveDialog::Saving(Some(saved))
            }
            (_, SaveDialogMsg::Plan(Err(error)) | SaveDialogMsg::Saved(Err(error))) => {
                SaveDialog::Failed(error)
            }
            (state, SaveDialogMsg::Save) => state,
        };
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let close = props.on_close.reform(|_| ());
        let keep_editing = html! {
            <Button variant={ButtonVariant::Link} label={t::KEEP_EDITING} onclick={close.clone()}/>
        };
        let (body, footer) = match self {
            SaveDialog::Planning => (html!(<p>{t::PLANNING}</p>), keep_editing),
            SaveDialog::Plan(plan) => {
                let save = ctx.link().callback(|_| SaveDialogMsg::Save);
                let footer = html! {
                    <>
                        <Button variant={ButtonVariant::Primary} label={t::SAVE} onclick={save}/>
                        {keep_editing}
                    </>
                };
                (preview(props, plan), footer)
            }
            SaveDialog::Saving(saved) => (saving(saved.as_ref()), html!()),
            SaveDialog::Failed(error) => (error.to_html(), keep_editing),
        };
        // a saved or failed save closes, the drafts are dropped or kept by then
        let footer = match self {
            SaveDialog::Saving(Some(saved)) if saved.state != SaveState::Saving => html! {
                <Button variant={ButtonVariant::Primary} label={t::CLOSE} onclick={close}/>
            },
            _ => footer,
        };
        html! {
            <div class="pf-v6-c-backdrop resident-preview">
                <Bullseye>
                    <Modal
                        title={t::PREVIEW_TITLE}
                        description={t::PREVIEW_DESCRIPTION}
                        variant={ModalVariant::Medium}
                        onclose={props.on_close.clone()}
                        disable_close_click_outside=true
                        {footer}
                    >
                        {body}
                    </Modal>
                </Bullseye>
            </div>
        }
    }
}

/// Saves the drafts and then asks how saving goes until it is through.
fn save(ctx: &Context<SaveDialog>, fingerprint: String) {
    let scope = ctx.link().clone();
    let variables = SaveChangeVariables {
        resident: ctx.props().resident.id,
        changes: ctx.props().drafts.changes(),
        fingerprint,
    };
    spawn_local(async move {
        let mut saved = request_resident(scope.clone(), SaveChange::build(variables))
            .await
            .map(|data| data.save_change);
        while let Ok(change) = &saved
            && change.state == SaveState::Saving
        {
            scope.send_message(SaveDialogMsg::Saved(Ok(change.clone())));
            TimeoutFuture::new(1_000).await;
            let job = change.job;
            saved = request_resident(
                scope.clone(),
                SavedChangeQuery::build(SavedChangeVariables { job }),
            )
            .await
            // none once the provisioner forgot the job (restarted)
            .and_then(|data| data.saved_change.ok_or(FrontendError::MissingData));
        }
        scope.send_message(SaveDialogMsg::Saved(saved));
    });
}

fn port_at(port: &InterfaceRef) -> PortAt<'_> {
    PortAt {
        port: &port.name,
        device: &port.device.name,
    }
}

/// The plan in words: networks of ports, cables, rooms, WLANs; who is away meanwhile; what
/// the devices at it notice and which devices change.
fn preview(props: &SaveDialogProps, plan: &ResidentPlan) -> Html {
    let resident = &props.resident;
    let drafts = &props.drafts;
    let network = |vlan: Option<i32>| match vlan {
        Some(vlan) => resident
            .networks
            .iter()
            .find(|network| network.id == vlan)
            .map(|network| network.name.clone())
            .unwrap_or_else(|| t::FOREIGN_NETWORK.into()),
        None => t::NO_NETWORK.into(),
    };
    // a port's network by the row the resident changed it at: a socket's is the switch port
    // behind the wall, which the plan names
    let ports = drafts.port_vlans.iter().filter_map(|(key, vlan)| {
        let port = resident
            .ports
            .iter()
            .find(|port| PortKey::of(port) == *key)?;
        let title = port.label.as_deref().unwrap_or(&port.name);
        let before = match (&port.network, port.foreign_network) {
            (Some(network), _) => network.name.clone(),
            (None, true) => t::FOREIGN_NETWORK.into(),
            (None, false) => t::NO_NETWORK.into(),
        };
        Some(t::port_network_change(title, &before, &network(*vlan)))
    });
    let removed = plan
        .cables_removed
        .iter()
        .map(|cable| t::cable_removed(port_at(&cable.port), cable.far.as_ref().map(port_at)));
    let added = plan
        .cables_added
        .iter()
        .map(|cable| t::cable_added(port_at(&cable.a), port_at(&cable.b)));
    let rooms = plan.rooms.iter().map(|room| {
        let path = |location: &crate::graphql::resident::change::LocationRef| {
            t::location_path(&location.path)
        };
        let to = room.location.as_ref().map(path).unwrap_or_default();
        let before = room.location_before.as_ref().map(path);
        t::room_moved(&room.device.name, &to, before.as_deref())
    });
    let wlans = plan.wlans.iter().flat_map(|wlan| {
        let ssid = wlan
            .ssid
            .as_deref()
            .map(|ssid| t::ssid_change(&wlan.ssid_before, ssid));
        let psk = wlan.psk_set.then(|| t::psk_change(&wlan.ssid_before));
        let enabled = wlan.enabled.map(|enabled| match enabled {
            true => t::wlan_on(&wlan.ssid_before),
            false => t::wlan_off(&wlan.ssid_before),
        });
        ssid.into_iter().chain(psk).chain(enabled)
    });
    let lines = ports
        .chain(removed)
        .chain(added)
        .chain(rooms)
        .chain(wlans)
        .collect::<Vec<_>>();
    // a device taken off its link is away for good, as its hint says
    let leaving = drafts
        .links
        .iter()
        .filter(|(_, link)| matches!(link, LinkDraft::Disconnect))
        .map(|(device, _)| *device)
        .collect::<Vec<_>>();
    let cut_off = plan
        .cut_off
        .iter()
        .filter(|device| !leaving.contains(&device.id))
        .map(|device| device.name.as_str())
        .collect::<Vec<_>>();
    let cut_off = (!cut_off.is_empty()).then(
        || html!(<Alert inline=true r#type={AlertType::Warning} title={t::cut_off(&cut_off)}/>),
    );
    html! {
        <div class="resident-edit">
            <ul class="resident-facts">
                { for lines.into_iter().map(|line| html!(<li>{line}</li>)) }
            </ul>
            {cut_off}
            {devices_alert(plan, &hints(drafts, plan))}
        </div>
    }
}

/// What the devices at the changed elements notice, once per kind.
fn hints(drafts: &Drafts, plan: &ResidentPlan) -> Vec<&'static str> {
    let ports = (!drafts.port_vlans.is_empty()).then(|| {
        match drafts.port_vlans.values().any(Option::is_none) {
            true => t::PORT_CLIENTS_CUT,
            false => t::PORT_CLIENTS_MOVE,
        }
    });
    let links = drafts.links.values().map(|link| match link {
        LinkDraft::Connect { seen: false, .. } => t::LINK_HINT,
        LinkDraft::Connect { seen: true, .. } => t::TAKE_HINT,
        LinkDraft::Disconnect => t::UNLINK_HINT,
    });
    // switched on, none are connected yet
    let wlans = if plan.wlans.iter().any(|wlan| wlan.enabled == Some(false)) {
        Some(t::CLIENTS_DISCONNECT)
    } else if plan
        .wlans
        .iter()
        .any(|wlan| wlan.ssid.is_some() || wlan.psk_set)
    {
        Some(t::CLIENTS_RECONNECT)
    } else {
        None
    };
    let mut hints = Vec::new();
    for hint in ports.into_iter().chain(links).chain(wlans) {
        if !hints.contains(&hint) {
            hints.push(hint);
        }
    }
    hints
}

/// The devices whose configuration the plan changes, and what the devices at it notice; a plan
/// changing no device's configuration says it changes NetBox only.
fn devices_alert(plan: &ResidentPlan, hints: &[&str]) -> Html {
    let devices = plan
        .devices
        .iter()
        .map(|device| device.name.as_str())
        .collect::<Vec<_>>();
    let title = if devices.is_empty() {
        t::NETBOX_ONLY.to_string()
    } else {
        t::devices_change(&devices)
    };
    html! {
        <Alert inline=true r#type={AlertType::Info} {title}>
            { for hints.iter().map(|hint| html!(<p>{*hint}</p>)) }
        </Alert>
    }
}

/// How saving goes: saving, saved (the devices get it from the rollout) or why it failed.
fn saving(saved: Option<&SavedChange>) -> Html {
    match saved.map(|saved| (&saved.state, saved.error.as_ref())) {
        None | Some((SaveState::Saving, _)) => html!(<p>{t::SAVING}</p>),
        Some((SaveState::Saved, _)) => html! {
            <Alert inline=true r#type={AlertType::Success} title={t::SAVED}>
                <p>{t::SAVED_HINT}</p>
            </Alert>
        },
        Some((_, error)) => {
            let text = error
                .map(|error| i18n::error::server(&ServerProblem::from(error)))
                .unwrap_or_default();
            html! {
                <Alert inline=true r#type={AlertType::Danger} title={t::NOT_SAVED}>
                    <p>{text}</p>
                </Alert>
            }
        }
    }
}
