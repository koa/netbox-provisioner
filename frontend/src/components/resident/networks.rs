//! Scope: The tab „Netze“: a card per network with its WLAN's switch, name and password,
//! changed right there as a draft, and the devices in it; not keeping the drafts
//! (`components::resident`), nor saving them (`save_dialog`).
use super::{
    drafts::{Drafts, WlanDraft},
    network_devices::NetworkDevicesView,
    ports,
    qr_dialog::QrDialog,
    status::State,
};
use crate::graphql::resident::residents::{PortKind, Resident, ResidentNetwork, ResidentWlan};
use i18n::resident as t;
use patternfly_yew::prelude::{
    Button, ButtonVariant, Content, Form, FormGroup, FormHelperText, InputState, Switch, TextInput,
    TextInputType,
};
use yew::{Callback, Component, Context, Html, Properties, html, html_nested};

/// A WLAN's draft as the page keeps it, or none to drop it.
#[derive(Debug, Clone, PartialEq)]
pub struct WlanEdit {
    pub wlan: i32,
    pub draft: WlanDraft,
}

/// The networks, each with its WLAN and devices.
pub fn networks(
    resident: &Resident,
    drafts: &Drafts,
    state: State,
    edit: &Callback<WlanEdit>,
) -> Html {
    if resident.networks.is_empty() {
        return html!(<Content><p>{t::NO_NETWORKS}</p></Content>);
    }
    let cards = resident.networks.iter().map(|network| {
        let ssid = network.wlans.first().map(|wlan| wlan.ssid.as_str());
        let title = t::network_card(&t::network_title(&network.name, ssid));
        let changed = network
            .wlans
            .iter()
            .any(|wlan| drafts.wlans.contains_key(&wlan.id));
        let reset = {
            let edit = edit.clone();
            let wlans = network.wlans.iter().map(|wlan| wlan.id).collect::<Vec<_>>();
            Callback::from(move |_| {
                for wlan in &wlans {
                    edit.emit(WlanEdit {
                        wlan: *wlan,
                        draft: WlanDraft::default(),
                    });
                }
            })
        };
        let wlans = if network.wlans.is_empty() {
            html!(<p>{t::NO_WLAN}</p>)
        } else {
            network
                .wlans
                .iter()
                .map(|wlan| {
                    html! {
                        <WlanFields
                            key={wlan.id}
                            wlan={wlan.clone()}
                            draft={drafts.wlans.get(&wlan.id).cloned().unwrap_or_default()}
                            edit={edit.clone()}
                        />
                    }
                })
                .collect()
        };
        let heading = html!(<h2 class="pf-v6-c-card__title-text">{title}</h2>);
        let status = state.network(network);
        let text = status
            .as_ref()
            .filter(|_| !changed)
            .and_then(|status| status.text());
        html_nested! {
            <div class="pf-v6-c-card" key={network.id}>
                {super::card_header(heading, changed, reset, status.as_ref())}
                <div class="pf-v6-c-card__body resident-network">
                    <div class="resident-wlan">
                        {text}
                        {wlans}
                        {cabled(resident, network)}
                    </div>
                    <NetworkDevicesView resident={resident.id} vlan={network.id}/>
                </div>
            </div>
        }
    });
    html!(<div class="resident-networks">{ for cards }</div>)
}

/// „Per Kabel an“: the resident's sockets and ports with the network as saved, a socket by its
/// name, a port with its device, as the tab „Dosen und Geräte“ names them.
fn cabled(resident: &Resident, network: &ResidentNetwork) -> Html {
    let names = resident
        .ports
        .iter()
        .filter(|port| !port.foreign_network)
        .filter(|port| port.network.as_ref().is_some_and(|n| n.id == network.id))
        .map(|port| match port.kind {
            PortKind::Socket => ports::title(port),
            _ => ports::sub(port),
        })
        .collect::<Vec<_>>();
    let text = match names.is_empty() {
        true => t::NO_CABLED_PORTS.to_string(),
        false => names.join(", "),
    };
    html! {
        <dl class="pf-v6-c-description-list pf-m-compact">
            <div class="pf-v6-c-description-list__group">
                <dt class="pf-v6-c-description-list__term">
                    <span class="pf-v6-c-description-list__text">{t::CABLED_AT}</span>
                </dt>
                <dd class="pf-v6-c-description-list__description">
                    <div class="pf-v6-c-description-list__text">{text}</div>
                </dd>
            </div>
        </dl>
    }
}

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct WlanFieldsProps {
    pub wlan: ResidentWlan,
    pub draft: WlanDraft,
    pub edit: Callback<WlanEdit>,
}

/// A field being changed, with its text so far.
#[derive(Debug, Clone, PartialEq)]
pub enum Editing {
    Ssid(String),
    Psk(String),
}

/// A WLAN's switch, name and password: shown, and one of them changed at a time until it is
/// taken over as a draft; the password shown in clear or not.
pub struct WlanFields {
    editing: Option<Editing>,
    show_psk: bool,
    /// the QR code is shown
    qr: bool,
}

pub enum WlanFieldsMsg {
    Enabled(bool),
    Edit(Editing),
    Text(String),
    RandomPsk,
    Take,
    Cancel,
    ShowPsk,
    Qr(bool),
}

impl Component for WlanFields {
    type Message = WlanFieldsMsg;
    type Properties = WlanFieldsProps;

    fn create(_ctx: &Context<Self>) -> Self {
        WlanFields {
            editing: None,
            show_psk: false,
            qr: false,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let props = ctx.props();
        let wlan = &props.wlan;
        let mut draft = props.draft.clone();
        match msg {
            WlanFieldsMsg::Enabled(enabled) => {
                draft.enabled = (enabled != wlan.enabled).then_some(enabled);
                self.emit(ctx, draft);
            }
            WlanFieldsMsg::Edit(editing) => self.editing = Some(editing),
            WlanFieldsMsg::Text(text) => match &mut self.editing {
                Some(Editing::Ssid(ssid)) => *ssid = text,
                Some(Editing::Psk(psk)) => *psk = text,
                None => {}
            },
            WlanFieldsMsg::RandomPsk => {
                if let Some(psk) = random_psk() {
                    self.editing = Some(Editing::Psk(psk));
                }
            }
            WlanFieldsMsg::Take => {
                match self.editing.take() {
                    Some(Editing::Ssid(ssid)) if ssid_valid(&ssid) => {
                        draft.ssid = (ssid != wlan.ssid).then_some(ssid);
                    }
                    Some(Editing::Psk(psk)) if psk_valid(&psk) => {
                        draft.psk = (wlan.psk.as_ref() != Some(&psk)).then_some(psk);
                    }
                    editing => {
                        self.editing = editing;
                        return true;
                    }
                }
                self.emit(ctx, draft);
            }
            WlanFieldsMsg::Cancel => self.editing = None,
            WlanFieldsMsg::ShowPsk => self.show_psk = !self.show_psk,
            WlanFieldsMsg::Qr(open) => self.qr = open,
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        let props = ctx.props();
        let wlan = &props.wlan;
        let draft = &props.draft;
        let enabled = draft.enabled.unwrap_or(wlan.enabled);
        let ssid = draft.ssid.as_deref().unwrap_or(&wlan.ssid);
        let switch = html! {
            <Switch
                label={t::wlan_switch(ssid)}
                checked={enabled}
                onchange={link.callback(WlanFieldsMsg::Enabled)}
            />
        };
        if !enabled {
            return html! {
                <>
                    {switch}
                    <p class="pf-v6-c-helper-text">{t::WLAN_OFF_HINT}</p>
                </>
            };
        }
        let ssid_field = match &self.editing {
            Some(editing @ Editing::Ssid(_)) => self.edit_field(ctx, editing),
            _ => {
                let start = Editing::Ssid(ssid.to_string());
                html! {
                    <div class="resident-field">
                        <TextInput readonly=true value={ssid.to_string()}/>
                        <Button
                            variant={ButtonVariant::Control}
                            label={t::CHANGE}
                            onclick={link.callback(move |_| WlanFieldsMsg::Edit(start.clone()))}
                        />
                    </div>
                }
            }
        };
        let ssid_helper = matches!(self.editing, Some(Editing::Ssid(_)))
            .then(|| FormHelperText::from(t::SSID_HINT));
        // an open WLAN keeps its security: only an admin gives it a password
        let psk_field = wlan.psk.as_ref().map(|psk| {
            let psk = draft.psk.as_ref().unwrap_or(psk);
            let field = match &self.editing {
                Some(editing @ Editing::Psk(_)) => self.edit_field(ctx, editing),
                _ => {
                    let start = Editing::Psk(psk.clone());
                    let r#type = match self.show_psk {
                        true => TextInputType::Text,
                        false => TextInputType::Password,
                    };
                    let show = match self.show_psk {
                        true => t::HIDE_PSK,
                        false => t::SHOW_PSK,
                    };
                    html! {
                        <div class="resident-field">
                            <TextInput readonly=true {r#type} value={psk.clone()}/>
                            <Button
                                variant={ButtonVariant::Control}
                                label={show}
                                onclick={link.callback(|_| WlanFieldsMsg::ShowPsk)}
                            />
                            <Button
                                variant={ButtonVariant::Control}
                                label={t::CHANGE}
                                onclick={link.callback(move |_| WlanFieldsMsg::Edit(start.clone()))}
                            />
                        </div>
                    }
                }
            };
            let helper = matches!(self.editing, Some(Editing::Psk(_)))
                .then(|| FormHelperText::from(t::PSK_HINT));
            html! {
                <FormGroup label={t::PSK} helper_text={helper}>{field}</FormGroup>
            }
        });
        // with the drafts: the code a phone joins once they are rolled out
        let qr = self.qr.then(|| {
            html! {
                <QrDialog
                    ssid={ssid.to_string()}
                    psk={draft.psk.clone().or_else(|| wlan.psk.clone())}
                    unsaved={!draft.is_empty()}
                    on_close={link.callback(|()| WlanFieldsMsg::Qr(false))}
                />
            }
        });
        html! {
            <>
                {switch}
                <Form>
                    <FormGroup label={t::SSID} helper_text={ssid_helper}>{ssid_field}</FormGroup>
                    {psk_field}
                </Form>
                <div>
                    <Button
                        variant={ButtonVariant::Secondary}
                        label={t::QR_BUTTON}
                        onclick={link.callback(|_| WlanFieldsMsg::Qr(true))}
                    />
                </div>
                <p class="pf-v6-c-helper-text">{t::access_points(wlan.access_points)}</p>
                {qr}
            </>
        }
    }
}

impl WlanFields {
    /// The field being changed, taken over only when valid; a password can be made up.
    fn edit_field(&self, ctx: &Context<Self>, editing: &Editing) -> Html {
        let (text, valid, label) = match editing {
            Editing::Ssid(ssid) => (ssid, ssid_valid(ssid), t::SSID),
            Editing::Psk(psk) => (psk, psk_valid(psk), t::PSK),
        };
        let psk = matches!(editing, Editing::Psk(_));
        let link = ctx.link();
        let state = match valid {
            true => InputState::Default,
            false => InputState::Error,
        };
        let random = psk.then(|| {
            html! {
                <Button
                    variant={ButtonVariant::Secondary}
                    label={t::RANDOM_PSK}
                    onclick={link.callback(|_| WlanFieldsMsg::RandomPsk)}
                />
            }
        });
        html! {
            <div class="resident-edit">
                <TextInput
                    value={text.clone()}
                    placeholder={label}
                    {state}
                    autofocus=true
                    autocomplete="off"
                    onchange={link.callback(WlanFieldsMsg::Text)}
                />
                <div class="resident-edit-actions">
                    <Button
                        variant={ButtonVariant::Primary}
                        label={t::APPLY}
                        disabled={!valid}
                        onclick={link.callback(|_| WlanFieldsMsg::Take)}
                    />
                    {random}
                    <Button
                        variant={ButtonVariant::Link}
                        label={t::CANCEL}
                        onclick={link.callback(|_| WlanFieldsMsg::Cancel)}
                    />
                </div>
            </div>
        }
    }

    fn emit(&self, ctx: &Context<Self>, draft: WlanDraft) {
        let props = ctx.props();
        props.edit.emit(WlanEdit {
            wlan: props.wlan.id,
            draft,
        });
    }
}

/// Whether `ssid` can be a WLAN's name: 1 to 32 bytes, no space at either end (as the
/// provisioner checks it, which also refuses one another WLAN of the group has).
fn ssid_valid(ssid: &str) -> bool {
    !ssid.is_empty() && ssid.len() <= 32 && ssid.trim() == ssid
}

/// Whether `psk` can be a WPA passphrase here: 12 to 63 printable ASCII characters (as the
/// provisioner checks it).
fn psk_valid(psk: &str) -> bool {
    (12..=63).contains(&psk.len()) && psk.chars().all(|c| c.is_ascii_graphic() || c == ' ')
}

/// A password of 16 characters from the browser's random source, without characters one
/// mixes up when typing it off a screen (0 O, 1 l I); none without that source.
fn random_psk() -> Option<String> {
    const CHARACTERS: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    const LENGTH: usize = 16;
    // bytes below the largest multiple of the characters only, so each is as likely
    let limit = 256 - 256 % CHARACTERS.len();
    let crypto = web_sys::window()?.crypto().ok()?;
    let mut psk = String::with_capacity(LENGTH);
    while psk.len() < LENGTH {
        let mut bytes = [0u8; 32];
        crypto.get_random_values_with_u8_array(&mut bytes).ok()?;
        psk.extend(
            bytes
                .iter()
                .map(|byte| usize::from(*byte))
                .filter(|byte| *byte < limit)
                .map(|byte| char::from(CHARACTERS[byte % CHARACTERS.len()]))
                .take(LENGTH - psk.len()),
        );
    }
    Some(psk)
}
