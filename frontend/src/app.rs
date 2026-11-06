//! The frame of every page: the login's settings from the backend, the OIDC session, the router,
//! and each route's page below the breadcrumb.
use crate::{
    components::{
        login::Login, login_expired::LoginExpired, menu::Navigation, network_map::NetworkMap,
    },
    error::FrontendError,
    graphql::{
        anonymous::settings::{Authentication, Settings},
        request_anonymous,
    },
    i18n,
    pages::{
        devices::{Devices, show::ShowDevice},
        sites::Sites,
    },
    routes::{AppRoute, DeviceView, SiteView},
};
use cynic::QueryBuilder;
use patternfly_yew::prelude::{
    BackdropViewer, Bullseye, PageSection, PageSectionType, Spinner, ToastViewer,
};
use yew::{Component, Context, Html, Properties, function_component, html, platform::spawn_local};
use yew_nested_router::{Router, prelude::Switch as RouterSwitch};
use yew_oauth2::{
    openid::{Config, OAuth2},
    prelude::Authenticated,
};

/// Where the provider for the login is, from the backend (`authentication`).
#[derive(Debug, Clone, PartialEq)]
pub struct AuthenticationData {
    client_id: String,
    issuer_url: String,
    scopes: Vec<String>,
}

#[derive(Debug)]
pub enum App {
    Pending,
    Loaded(AuthenticationData),
    Failed(FrontendError),
}
#[derive(Debug)]
pub enum AppMessage {
    AuthenticationData(AuthenticationData),
    Error(FrontendError),
}

impl Component for App {
    type Message = AppMessage;
    type Properties = ();
    fn create(_ctx: &Context<Self>) -> Self {
        App::Pending
    }
    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        *self = match msg {
            AppMessage::AuthenticationData(config) => App::Loaded(config),
            AppMessage::Error(e) => App::Failed(e),
        };
        true
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        match self {
            App::Failed(error) => html!(<Bullseye>{error.to_html()}</Bullseye>),
            App::Loaded(config) => html!(<MainOAuth2 config={config.clone()}/>),
            App::Pending => html!(<Bullseye><Spinner/></Bullseye>),
        }
    }
    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            let scope = ctx.link().clone();
            spawn_local(async move {
                scope.send_message(match request_anonymous(Settings::build(())).await {
                    Ok(Settings {
                        authentication:
                            Authentication {
                                client_id,
                                issuer_url,
                                scopes,
                            },
                    }) => AppMessage::AuthenticationData(AuthenticationData {
                        client_id,
                        issuer_url,
                        scopes,
                    }),
                    Err(error) => AppMessage::Error(error),
                });
            });
        }
    }
}
#[derive(Properties, Clone, PartialEq, Debug)]
pub struct MainOAuth2Props {
    config: AuthenticationData,
}
#[function_component(MainOAuth2)]
pub fn main_oauth2(props: &MainOAuth2Props) -> Html {
    let config = Config::new(
        props.config.client_id.clone(),
        props.config.issuer_url.clone(),
    );
    let scopes = props.config.scopes.clone();
    html! {
     <OAuth2 {config} {scopes}>
        <Authenticated>
            <Router<AppRoute> default={AppRoute::default()}>
                <MainPage/>
            </Router<AppRoute>>
        </Authenticated>
        <Login/>
      </OAuth2>
    }
}

#[function_component(MainPage)]
fn main_page() -> Html {
    html! {
        <BackdropViewer>
            <ToastViewer>
                    <LoginExpired/>
                    <RouterSwitch<AppRoute> render={content}/>
            </ToastViewer>
        </BackdropViewer>
    }
}

/// The page of `route` in PatternFly's page layout, as cable-editor does: the breadcrumb in a
/// `PageSection` inside `<main>`, followed by the page. There is no masthead or sidebar, and
/// the document scrolls instead of the main container (`.app-page` in `local.css`).
fn content(route: AppRoute) -> Html {
    let (site, device) = match &route {
        AppRoute::Site {
            id,
            view: SiteView::Device { id: device, .. },
        } => (Some(*id), Some(*device)),
        AppRoute::Site { id, .. } => (Some(*id), None),
        AppRoute::Sites | AppRoute::NetworkMap | AppRoute::NotFound => (None, None),
    };
    let content = match route {
        AppRoute::Sites => html! {<Sites/>},
        AppRoute::NetworkMap => html! {<NetworkMap site={None}/>},
        AppRoute::Site { id, view } => site_content(view, id),
        AppRoute::NotFound => html! {<h1>{i18n::pages::NOT_FOUND}</h1>},
    };
    html! {
        <div class="pf-v6-c-page pf-m-no-sidebar app-page">
            <div class="pf-v6-c-page__main-container">
                <main class="pf-v6-c-page__main" id="main-content" tabindex="-1">
                    <PageSection r#type={PageSectionType::Breadcrumbs}>
                        <div class="breadcrumb-bar"><Navigation {site} {device}/></div>
                    </PageSection>
                    <PageSection>{content}</PageSection>
                </main>
            </div>
        </div>
    }
}

fn site_content(view: SiteView, site: u32) -> Html {
    match view {
        // keyed: another site or device in the breadcrumb loads the page anew
        SiteView::Devices => html! {<Devices key={site} {site}/>},
        SiteView::Map => html! {<NetworkMap key={site} site={Some(site)}/>},
        SiteView::Device { id, view } => device_content(view, id),
    }
}

fn device_content(view: DeviceView, id: u32) -> Html {
    match view {
        DeviceView::Show => html! {<ShowDevice key={id} {id}/>},
    }
}
