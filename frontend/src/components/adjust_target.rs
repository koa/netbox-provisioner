use crate::error::FrontendError;
use crate::graphql::authenticated::{AdjustTargetListCredentials, adjust_target_list_credentials};
use crate::graphql::query_authenticated_response;
use log::{error, info};
use patternfly_yew::prelude::{
    Form, FormGroup, FormGroupValidated, InputState, SimpleSelect, TextInput, ValidationContext,
    ValidationResult, Validator,
};
use std::net::IpAddr;
use yew::html::Scope;
use yew::platform::spawn_local;
use yew::{Callback, Component, Context, Html, Properties, html};

#[derive(Debug, Clone, PartialOrd, PartialEq, Default)]
pub enum SelectedCredentials {
    #[default]
    Default,
    Named(Box<str>),
    Adhoc {
        username: Box<str>,
        password: Box<str>,
    },
}

#[derive(Debug, Clone, PartialOrd, PartialEq, Default)]
pub struct SelectedTarget {
    pub address: Option<IpAddr>,
    pub credentials: SelectedCredentials,
}

#[derive(Debug)]
pub struct AdjustTarget {
    callback: Callback<SelectedTarget>,
    available_credentials: Box<[Box<str>]>,
    selected_credential: Box<str>,
    current_ip_address: Box<str>,
    adhoc_username: Box<str>,
    adhoc_password: Box<str>,
    last_sent_target: Option<SelectedTarget>,
}

#[derive(Properties, Clone, PartialEq)]
pub struct AdjustTargetProps {
    #[prop_or_default]
    pub value: SelectedTarget,
    #[prop_or_default]
    pub onchange: Callback<SelectedTarget>,
}

impl AdjustTarget {
    fn update_selected_target(&mut self) {
        let new_target_data = match self.build_selected_target() {
            Some(value) => value,
            None => return,
        };
        if Some(&new_target_data) != self.last_sent_target.as_ref() {
            self.callback.emit(new_target_data.clone());
            self.last_sent_target = Some(new_target_data);
        }
    }

    fn build_selected_target(&self) -> Option<SelectedTarget> {
        let trimmed_ip = self.current_ip_address.trim();
        let address = if trimmed_ip.is_empty() {
            None
        } else if let Ok(ip) = trimmed_ip.parse::<IpAddr>() {
            Some(ip)
        } else {
            return None;
        };
        let credentials = if !self.selected_credential.is_empty() {
            SelectedCredentials::Named(self.selected_credential.clone())
        } else if self.adhoc_password.is_empty() && self.adhoc_username.is_empty() {
            SelectedCredentials::Default
        } else {
            SelectedCredentials::Adhoc {
                username: self.adhoc_username.clone(),
                password: self.adhoc_password.clone(),
            }
        };
        Some(SelectedTarget {
            address,
            credentials,
        })
    }

    fn split_target(target: &SelectedTarget) -> (Box<str>, Box<str>, Box<str>, Box<str>) {
        let (selected_credential, adhoc_username, adhoc_password) = match &target.credentials {
            SelectedCredentials::Default => Default::default(),
            SelectedCredentials::Named(name) => (name.clone(), Box::default(), Box::default()),
            SelectedCredentials::Adhoc { username, password } => {
                (Box::default(), username.clone(), password.clone())
            }
        };
        let current_ip_address = target
            .address
            .map(|ip| ip.to_string().into_boxed_str())
            .unwrap_or_default();
        (
            selected_credential,
            adhoc_username,
            adhoc_password,
            current_ip_address,
        )
    }
}

pub enum AdjustTargetMsg {
    UpdateAddress(Box<str>),
    UpdateSelectedCredential(Box<str>),
    UpdateUsername(String),
    UpdatePassword(String),
    AvailableCredentials(Box<[Box<str>]>),
}

impl Component for AdjustTarget {
    type Message = AdjustTargetMsg;
    type Properties = AdjustTargetProps;

    fn create(ctx: &Context<Self>) -> Self {
        let props = ctx.props();
        let (selected_credential, adhoc_username, adhoc_password, current_ip_address) =
            Self::split_target(&props.value);
        Self {
            callback: props.onchange.clone(),
            available_credentials: Box::new([]),
            selected_credential,
            current_ip_address,
            adhoc_username,
            adhoc_password,
            last_sent_target: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            AdjustTargetMsg::UpdateAddress(address) => {
                self.current_ip_address = address;
                self.update_selected_target();
                true
            }
            AdjustTargetMsg::UpdateSelectedCredential(selected_credential) => {
                self.selected_credential = selected_credential;
                self.update_selected_target();
                true
            }
            AdjustTargetMsg::UpdateUsername(username) => {
                self.adhoc_username = username.into_boxed_str();
                self.update_selected_target();
                true
            }
            AdjustTargetMsg::UpdatePassword(password) => {
                self.adhoc_password = password.into_boxed_str();
                self.update_selected_target();
                true
            }
            AdjustTargetMsg::AvailableCredentials(list) => {
                self.available_credentials = list;
                self.update_selected_target();
                true
            }
        }
    }

    fn changed(&mut self, ctx: &Context<Self>, _old_props: &Self::Properties) -> bool {
        let props = ctx.props();
        let mut modified = false;
        if props.onchange != self.callback {
            self.callback = props.onchange.clone();
            modified = true;
        }
        if self.last_sent_target.as_ref() != Some(&props.value) {
            let (selected_credential, adhoc_username, adhoc_password, current_ip_address) =
                Self::split_target(&props.value);
            if selected_credential != self.selected_credential {
                self.selected_credential = selected_credential;
                modified = true;
            }
            if self.adhoc_username != adhoc_username {
                self.adhoc_username = adhoc_username;
                modified = true;
            }
            if self.adhoc_password != adhoc_password {
                self.adhoc_password = adhoc_password;
                modified = true;
            }
            if self.current_ip_address != current_ip_address {
                self.current_ip_address = current_ip_address;
                modified = true;
            }
            self.update_selected_target();
        }
        modified
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let ip_change = {
            let scope = ctx.link().clone();
            Callback::from(move |value: String| {
                scope.send_message(AdjustTargetMsg::UpdateAddress(value.into_boxed_str()));
            })
        };
        let ip_address = self.current_ip_address.to_string();
        let ip_validator = {
            Validator::from(|ctx: ValidationContext<String>| {
                let address = ctx.value.trim();
                if address.is_empty() {
                    ValidationResult::default()
                } else {
                    match ctx.value.parse::<IpAddr>() {
                        Ok(_) => ValidationResult {
                            message: None,
                            state: InputState::Success,
                        },
                        Err(e) => ValidationResult::error(format!("{}", e)),
                    }
                }
            })
        };
        let mut defined_credentials = Vec::with_capacity(self.available_credentials.len() + 1);
        defined_credentials.push(Box::from(""));
        defined_credentials.extend(self.available_credentials.iter().cloned());
        let selected_credential = self.selected_credential.clone();
        let on_change_credential = ctx
            .link()
            .callback(AdjustTargetMsg::UpdateSelectedCredential);
        let enable_adhoc = selected_credential.is_empty();
        let on_change_username = ctx.link().callback(AdjustTargetMsg::UpdateUsername);
        let on_change_password = ctx.link().callback(AdjustTargetMsg::UpdatePassword);
        let username = self.adhoc_username.to_string();
        let password = self.adhoc_password.to_string();

        html! {
            <Form>
                <FormGroupValidated<TextInput>
                    label="IP Address"
                    required=true
                    validator={ip_validator}>
                    <TextInput
                        value={ip_address}
                        onchange={ip_change}
                        placeholder="Alternate IP Address"
                    />
                </FormGroupValidated<TextInput>>
                <FormGroup label="Defined Credentials">
                    <SimpleSelect<Box<str>> selected={selected_credential} entries={defined_credentials} onselect={on_change_credential}/>
                </FormGroup>
                <FormGroup label="Adhoc Username">
                    <TextInput disabled={!enable_adhoc} onchange={on_change_username} value={username} />
                </FormGroup>
                <FormGroup label="Adhoc Password">
                    <TextInput disabled={!enable_adhoc} onchange={on_change_password} value={password} />
                </FormGroup>
            </Form>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            let scope = ctx.link().clone();
            spawn_local(async move {
                match fetch_known_credentials(scope.clone()).await {
                    Ok(list) => scope.send_message(AdjustTargetMsg::AvailableCredentials(list)),
                    Err(e) => {
                        error!("Failed to fetch available credentials. {}", e)
                    }
                }
            })
        }
    }
}

async fn fetch_known_credentials(
    scope: Scope<AdjustTarget>,
) -> Result<Box<[Box<str>]>, FrontendError> {
    let response = query_authenticated_response::<AdjustTargetListCredentials, _>(
        scope.clone(),
        adjust_target_list_credentials::Variables {},
    )
    .await?;
    Ok(response
        .data
        .into_iter()
        .flat_map(|d| d.list_credentials.into_iter())
        .map(Box::from)
        .collect())
}
