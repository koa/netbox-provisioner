//! Scope: The download of the root CA of WebFig over HTTPS; nothing else.
//!
//! A link to `/www-ca.crt` (the backend's, public), shown only with `www_ca` in the config.
use crate::{
    graphql::{authenticated::www_ca::WwwCa as WwwCaQuery, request},
    i18n::pages as t,
};
use cynic::QueryBuilder;
use patternfly_yew::prelude::{Alert, AlertType};
use yew::{Component, Context, Html, html, platform::spawn_local};

pub struct WwwCa {
    offered: bool,
}

pub enum WwwCaMsg {
    Offered(bool),
}

impl Component for WwwCa {
    type Message = WwwCaMsg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let scope = ctx.link().clone();
        spawn_local(async move {
            // without the answer the link stays away, as without `www_ca`
            let offered = request(scope.clone(), WwwCaQuery::build(()))
                .await
                .is_ok_and(|data| data.www_ca);
            scope.send_message(WwwCaMsg::Offered(offered));
        });
        Self { offered: false }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            WwwCaMsg::Offered(offered) => self.offered = offered,
        }
        true
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        if !self.offered {
            return html!();
        }
        html! {
            <Alert inline=true plain=true title={t::WWW_CA} r#type={AlertType::Info}>
                <p><a href="/www-ca.crt" download="webfig-ca.crt">{"webfig-ca.crt"}</a></p>
                <p>{t::WWW_CA_HINT}</p>
            </Alert>
        }
    }
}
