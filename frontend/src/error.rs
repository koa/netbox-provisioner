use std::error::Error;
use std::fmt::{Debug, Display, Formatter};

use log::error;
use patternfly_yew::prelude::{Alert, AlertGroup, AlertType};
use reqwest::header::InvalidHeaderValue;
use thiserror::Error;
use wasm_bindgen::JsValue;
use yew::{Html, ToHtml, html};

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
    #[error("Invalid http header")]
    InvalidHeader(#[from] InvalidHeaderValue),
    #[error("No data received")]
    MissingData,
}

impl ToHtml for FrontendError {
    fn to_html(&self) -> Html {
        match self {
            FrontendError::JS(js_error) => {
                html! {
                    <AlertGroup>
                        <Alert inline=true title="Javascript Error" r#type={AlertType::Danger}>{js_error.to_string()}</Alert>
                    </AlertGroup>
                }
            }
            FrontendError::Serde(serde_error) => {
                html! {
                    <AlertGroup>
                        <Alert inline=true title="Serialization Error" r#type={AlertType::Danger}>{serde_error.to_string()}</Alert>
                    </AlertGroup>
                }
            }
            FrontendError::Graphql(graphql_error) => {
                let graphql_error = graphql_error.clone();
                html! {
                    <AlertGroup>
                        <Alert inline=true title="Error from Server" r#type={AlertType::Danger}>
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
                                    html!{<li>{message}{" at "}{path}</li>}
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
                        <Alert inline=true title="Cannot call Server" r#type={AlertType::Danger}>{reqwest_error.to_string()}</Alert>
                    </AlertGroup>
                }
            }
            FrontendError::InvalidHeader(header_error) => {
                html! {
                    <AlertGroup>
                        <Alert inline=true title="Header Error" r#type={AlertType::Danger}>{header_error.to_string()}</Alert>
                    </AlertGroup>
                }
            }
            FrontendError::MissingData => {
                html! {
                    <AlertGroup>
                        <Alert inline=true title="Missing Data" r#type={AlertType::Danger}>{"No data received from server"}</Alert>
                    </AlertGroup>
                }
            }
        }
    }
}
