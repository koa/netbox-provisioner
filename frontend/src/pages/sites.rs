use crate::{
    i18n,
    components::{
        findings::{self, FindingsList},
        saved_jobs::SavedJobs,
    },
    error::FrontendError,
    graphql::{
        authenticated::{ListSites, list_sites},
        query_authenticated,
    },
    pages::routes::{AppRoute, SiteView},
};
use patternfly_yew::prelude::{Card, CardBody, CardHeader, CardTitle, Spinner};
use yew::{Component, Context, Html, html, platform::spawn_local};
use yew_nested_router::components::Link;

/// The sites with devices the provisioner manages, the first step to a device.
pub struct Sites {
    sites: Option<Result<Vec<list_sites::ListSitesTopologyAllSites>, FrontendError>>,
}

pub enum SitesMsg {
    Sites(Result<Vec<list_sites::ListSitesTopologyAllSites>, FrontendError>),
}

impl Component for Sites {
    type Message = SitesMsg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self { sites: None }
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
                            .map(|f| matches!(f.severity, list_sites::Severity::ERROR)),
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
                <SavedJobs/>
                <FindingsList site={None::<u32>}/>
                <div class="site-list">{content}</div>
            </>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            let scope = ctx.link().clone();
            spawn_local(async move {
                let sites =
                    query_authenticated::<ListSites, _>(scope.clone(), list_sites::Variables {})
                        .await
                        .map(|data| data.topology.all_sites);
                scope.send_message(SitesMsg::Sites(sites));
            });
        }
    }
}
