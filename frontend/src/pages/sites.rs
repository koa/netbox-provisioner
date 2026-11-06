use crate::{
    i18n,
    components::{
        findings::{self, FindingsList},
        saved_jobs::SavedJobs,
    },
    error::FrontendError,
    graphql::{
        list_devices::{ListSites, Site},
        live::{Subscription, watch_data},
        types::Severity,
    },
    pages::routes::{AppRoute, SiteView},
};
use cynic::SubscriptionBuilder;
use patternfly_yew::prelude::{Card, CardBody, CardHeader, CardTitle, Spinner};
use yew::{Component, Context, Html, html};
use yew_nested_router::components::Link;

/// The sites with devices the provisioner manages, the first step to a device.
pub struct Sites {
    sites: Option<Result<Vec<Site>, FrontendError>>,
    /// shown again when NetBox changed
    _subscription: Subscription,
}

pub enum SitesMsg {
    Sites(Result<Vec<Site>, FrontendError>),
}

impl Component for Sites {
    type Message = SitesMsg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let scope = ctx.link().clone();
        let subscription = watch_data(ctx.link(), ListSites::build(()), move |data| {
            scope.send_message(SitesMsg::Sites(data.map(|data| data.topology.all_sites)));
        });
        Self {
            sites: None,
            _subscription: subscription,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            SitesMsg::Sites(sites) => self.sites = Some(sites),
        }
        true
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        let content = match &self.sites {
            None => html!(<Spinner/>),
            Some(Err(error)) => error.to_html(),
            Some(Ok(sites)) => sites
                .iter()
                .map(|site| {
                    let to = AppRoute::Site {
                        id: site.id as u32,
                        view: SiteView::Devices,
                    };
                    let count = i18n::pages::device_count(site.devices.len());
                    let findings = findings::count(
                        site.findings
                            .iter()
                            .map(|f| matches!(f.severity, Severity::Error)),
                    )
                    .map(|count| html!(<div class="site-findings">{count}</div>));
                    html! {
                        <Card key={site.id}>
                            <CardHeader><CardTitle>
                                <Link<AppRoute> {to}>{site.name.clone()}</Link<AppRoute>>
                            </CardTitle></CardHeader>
                            <CardBody>{count}{findings}</CardBody>
                        </Card>
                    }
                })
                .collect(),
        };
        html! {
            <>
                <p class="site-links">
                    <Link<AppRoute> to={AppRoute::NetworkMap}>{i18n::network_map::OPEN_ALL}</Link<AppRoute>>
                </p>
                <SavedJobs/>
                <FindingsList site={None::<u32>}/>
                <div class="site-list">{content}</div>
            </>
        }
    }
}
