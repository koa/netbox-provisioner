//! Scope: The residents' page: a resident's own network, or for an admin the support view of a
//! chosen resident (`/residents`); not the view itself (`components::resident`).
use crate::{
    components::{resident::ResidentView, widgets::popup::Select},
    error::FrontendError,
    graphql::{
        request_resident,
        resident::residents::{Resident, Residents},
    },
};
use cynic::QueryBuilder;
use i18n::resident as t;
use patternfly_yew::prelude::{Alert, AlertType, Content, Level, Spinner, Title};
use std::fmt::{Display, Formatter};
use yew::{Component, Context, Html, Properties, html, platform::spawn_local};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct ResidentsPageProps {
    /// the admins' support view: one resident to pick; else the resident's own page
    #[prop_or_default]
    pub support: bool,
}

pub struct ResidentsPage {
    residents: Option<Result<Vec<Resident>, FrontendError>>,
    /// the support view's pick
    picked: Option<i32>,
}

pub enum ResidentsMsg {
    Residents(Result<Vec<Resident>, FrontendError>),
    Pick(i32),
}

/// A resident as an entry of the support view's select.
#[derive(Debug, Clone, PartialEq)]
struct Pick {
    id: i32,
    name: String,
}
impl Display for Pick {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

impl Component for ResidentsPage {
    type Message = ResidentsMsg;
    type Properties = ResidentsPageProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            residents: None,
            picked: None,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ResidentsMsg::Residents(residents) => {
                if self.picked.is_none() {
                    self.picked = residents
                        .as_ref()
                        .ok()
                        .and_then(|residents| residents.first())
                        .map(|resident| resident.id);
                }
                self.residents = Some(residents);
            }
            ResidentsMsg::Pick(id) => self.picked = Some(id),
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let residents = match &self.residents {
            None => return html!(<Spinner/>),
            Some(Err(error)) => return error.to_html(),
            Some(Ok(residents)) => residents,
        };
        if ctx.props().support {
            self.support(ctx, residents)
        } else {
            own(residents)
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            let scope = ctx.link().clone();
            spawn_local(async move {
                let residents = request_resident(scope.clone(), Residents::build(()))
                    .await
                    .map(|data| data.residents);
                scope.send_message(ResidentsMsg::Residents(residents));
            });
        }
    }
}

impl ResidentsPage {
    fn support(&self, ctx: &Context<Self>, residents: &[Resident]) -> Html {
        if residents.is_empty() {
            return html!(<Content><p>{t::NO_RESIDENTS}</p></Content>);
        }
        let entries: Vec<Pick> = residents
            .iter()
            .map(|resident| Pick {
                id: resident.id,
                name: resident.name.clone(),
            })
            .collect();
        let selected = entries
            .iter()
            .find(|entry| Some(entry.id) == self.picked)
            .cloned();
        let onselect = ctx
            .link()
            .callback(|pick: Pick| ResidentsMsg::Pick(pick.id));
        let view = residents
            .iter()
            .find(|resident| Some(resident.id) == self.picked)
            .map(|resident| html!(<ResidentView key={resident.id} resident={resident.clone()}/>));
        html! {
            <div class="resident-page">
                <Title level={Level::H1}>{t::SUPPORT_TITLE}</Title>
                <Alert inline=true r#type={AlertType::Warning} title={t::SUPPORT_HINT}>
                    <Select<Pick> placeholder={t::PICK_RESIDENT} {entries} {selected} {onselect}/>
                </Alert>
                {view}
            </div>
        }
    }
}

/// A resident's own page: each of its owners (a household mostly has one).
fn own(residents: &[Resident]) -> Html {
    let several = residents.len() > 1;
    let views = residents.iter().map(|resident| {
        let name = several.then(|| html!(<Title level={Level::H2}>{resident.name.clone()}</Title>));
        html! {
            <section key={resident.id}>
                {name}
                <ResidentView resident={resident.clone()}/>
            </section>
        }
    });
    html! {
        <div class="resident-page">
            <Title level={Level::H1}>{t::MY_NETWORK}</Title>
            <Content><p>{t::INTRO}</p></Content>
            { for views }
        </div>
    }
}
