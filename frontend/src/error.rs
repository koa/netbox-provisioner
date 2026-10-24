use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

use crate::{
    graphql::authenticated::{device_overview, job_events, site_diff_events},
    i18n::error as t,
};
use patternfly_yew::prelude::{Alert, AlertGroup, AlertType};
use reqwest::header::InvalidHeaderValue;
use thiserror::Error;
use wasm_bindgen::JsValue;
use yew::{Html, html};

/// An error the backend reports (its `ErrorInfo`: in a GraphQL error's extensions, a job's
/// `error`, a job event), worded by [`i18n::error::server`](crate::i18n::error::server).
#[derive(Clone, Debug, PartialEq)]
pub struct ServerProblem {
    pub code: String,
    pub params: Vec<(String, String)>,
    /// the backend's English text, for a code the frontend doesn't word yet
    pub message: String,
}

impl ServerProblem {
    /// the value of param `name`, empty without it
    pub fn param(&self, name: &str) -> &str {
        self.params
            .iter()
            .find(|(param, _)| param == name)
            .map(|(_, value)| value.as_str())
            .unwrap_or_default()
    }
    pub fn text(&self) -> String {
        t::server(self)
    }
    /// From a GraphQL error's extensions (`code`, `params`); without a code only its message.
    pub fn of(error: &graphql_client::Error) -> Self {
        let extensions = error.extensions.as_ref();
        let code = extensions
            .and_then(|extensions| extensions.get("code"))
            .and_then(|code| code.as_str())
            .unwrap_or_default()
            .to_string();
        let params = extensions
            .and_then(|extensions| extensions.get("params"))
            .and_then(|params| params.as_object())
            .map(|params| {
                params
                    .iter()
                    .map(|(name, value)| (name.clone(), value.as_str().unwrap_or_default().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        ServerProblem {
            code,
            params,
            message: error.message.clone(),
        }
    }
}

/// `ServerProblem` from the `Problem` fragment, which every query module has a type of.
macro_rules! problem_from {
    ($($fragment:ty),*) => {$(
        impl From<&$fragment> for ServerProblem {
            fn from(problem: &$fragment) -> Self {
                ServerProblem {
                    code: problem.code.clone(),
                    params: problem
                        .params
                        .iter()
                        .map(|param| (param.name.clone(), param.value.clone()))
                        .collect(),
                    message: problem.message.clone(),
                }
            }
        }
    )*};
}
problem_from!(job_events::Problem, device_overview::Problem, site_diff_events::Problem);

pub struct JavascriptError {
    original_value: JsValue,
}

impl JavascriptError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(string) = self.original_value.as_string() {
            f.write_str(&string)?;
        }
        Ok(())
    }
}

impl From<JsValue> for JavascriptError {
    fn from(value: JsValue) -> Self {
        JavascriptError {
            original_value: value,
        }
    }
}

impl Debug for JavascriptError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Self::fmt(self, f)
    }
}

impl Display for JavascriptError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Self::fmt(self, f)
    }
}

impl Error for JavascriptError {}

#[derive(Error, Debug)]
pub enum FrontendError {
    #[error("Generic Javascript error")]
    JS(#[from] JavascriptError),
    #[error("Cannot convert json")]
    Serde(#[from] serde_json::Error),
    #[error("Graphql Execution Error")]
    Graphql(Vec<graphql_client::Error>),
    #[error("Error on http request")]
    Reqwest(#[from] reqwest::Error),
    #[error("Server answered {status}: {text}")]
    Server { status: u16, text: String },
    #[error("Invalid http header")]
    InvalidHeader(#[from] InvalidHeaderValue),
    #[error("No data received")]
    MissingData,
}

impl FrontendError {
    /// One line for the user, where there is no room for [`Self::to_html`] (`Display` is for
    /// the log).
    pub fn message(&self) -> String {
        match self {
            FrontendError::JS(error) => format!("{}: {error}", t::BROWSER),
            FrontendError::Serde(error) => format!("{}: {error}", t::UNREADABLE),
            FrontendError::Graphql(errors) => format!(
                "{}: {}",
                t::SERVER,
                errors.iter().map(|error| ServerProblem::of(error).text()).collect::<Vec<_>>().join("; ")
            ),
            FrontendError::Reqwest(error) => format!("{}: {error}", t::UNREACHABLE),
            FrontendError::Server { status, text } => format!("{}: {text}", t::refused(*status)),
            FrontendError::InvalidHeader(error) => format!("{}: {error}", t::HEADER),
            FrontendError::MissingData => t::MISSING_DATA_TEXT.into(),
        }
    }
    pub fn to_html(&self) -> Html {
        match self {
            FrontendError::JS(js_error) => {
                html! {
                    <AlertGroup>
                        <Alert inline=true title={t::BROWSER} r#type={AlertType::Danger}><p>{js_error.to_string()}</p></Alert>
                    </AlertGroup>
                }
            }
            FrontendError::Serde(serde_error) => {
                html! {
                    <AlertGroup>
                        <Alert inline=true title={t::UNREADABLE} r#type={AlertType::Danger}><p>{serde_error.to_string()}</p></Alert>
                    </AlertGroup>
                }
            }
            FrontendError::Graphql(graphql_error) => {
                let graphql_error = graphql_error.clone();
                html! {
                    <AlertGroup>
                        <Alert inline=true title={t::SERVER} r#type={AlertType::Danger}>
                            <ul>
                        {
                          graphql_error.iter().map(|error| {
                                let message=&ServerProblem::of(error).text();
                                if let Some(path) = error
                                    .path.as_ref()
                                    .map(|p|
                                        p.iter()
                                            .map(|path| path.to_string())
                                            .collect::<Vec<String>>()
                                            .join("/")
                                    )
                                {
                                    html!{<li>{t::at(message, &path)}</li>}
                                }else{
                                    html!{<li>{message}</li>}
                                }
                            }).collect::<Html>()
                        }
                            </ul>
                        </Alert>
                    </AlertGroup>
                }
            }
            FrontendError::Reqwest(reqwest_error) => {
                html! {
                    <AlertGroup>
                        <Alert inline=true title={t::UNREACHABLE} r#type={AlertType::Danger}><p>{reqwest_error.to_string()}</p></Alert>
                    </AlertGroup>
                }
            }
            FrontendError::Server { status, text } => {
                html! {
                    <AlertGroup>
                        <Alert inline=true title={t::refused(*status)} r#type={AlertType::Danger}><p>{text}</p></Alert>
                    </AlertGroup>
                }
            }
            FrontendError::InvalidHeader(header_error) => {
                html! {
                    <AlertGroup>
                        <Alert inline=true title={t::HEADER} r#type={AlertType::Danger}><p>{header_error.to_string()}</p></Alert>
                    </AlertGroup>
                }
            }
            FrontendError::MissingData => {
                html! {
                    <AlertGroup>
                        <Alert inline=true title={t::MISSING_DATA} r#type={AlertType::Danger}><p>{t::MISSING_DATA_TEXT}</p></Alert>
                    </AlertGroup>
                }
            }
        }
    }
}

