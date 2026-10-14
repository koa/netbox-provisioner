use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

use crate::i18n::error as t;
use patternfly_yew::prelude::{Alert, AlertGroup, AlertType};
use reqwest::header::InvalidHeaderValue;
use thiserror::Error;
use wasm_bindgen::JsValue;
use yew::{Html, html};

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
                errors.iter().map(|error| error.message.as_str()).collect::<Vec<_>>().join("; ")
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
                                let message=&error.message;
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

