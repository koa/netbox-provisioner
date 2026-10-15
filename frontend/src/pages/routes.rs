use crate::components::menu::Navigation;
use crate::i18n;
use crate::pages::devices::Devices;
use crate::pages::devices::show::ShowDevice;
use crate::pages::sites::Sites;
use patternfly_yew::prelude::{PageSection, PageSectionType};
use yew::{Html, html};
use yew_nested_router::Target;

/// `/sites`, `/site/<site>` (its devices), `/site/<site>/device/<device>/show`
#[derive(Debug, Clone, Default, PartialEq, Eq, Target)]
pub enum AppRoute {
    NotFound,
    #[default]
    Sites,
    Site {
        id: u32,
        #[target(nested)]
        view: SiteView,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Target)]
pub enum SiteView {
    #[target(index)]
    Devices,
    Device {
        id: u32,
        #[target(nested)]
        view: DeviceView,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Target)]
pub enum DeviceView {
    Show,
}

impl AppRoute {
    pub fn device(site: u32, device: u32) -> Self {
        Self::Site {
            id: site,
            view: SiteView::Device {
                id: device,
                view: DeviceView::Show,
            },
        }
    }
    /// Renders the page in PatternFly's page layout, as cable-editor does: the breadcrumb in a
    /// `PageSection` inside `<main>`, followed by the page. There is no masthead or sidebar, and
    /// the document scrolls instead of the main container (`.app-page` in `local.css`).
    pub fn content(self) -> Html {
        let (site, device) = match &self {
            AppRoute::Site {
                id,
                view: SiteView::Device { id: device, .. },
            } => (Some(*id), Some(*device)),
            AppRoute::Site { id, .. } => (Some(*id), None),
            AppRoute::Sites | AppRoute::NotFound => (None, None),
        };
        let content = match self {
            AppRoute::Sites => html! {<Sites/>},
            AppRoute::Site { id, view } => view.content(id),
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
}

impl SiteView {
    pub fn content(self, site: u32) -> Html {
        match self {
            // keyed: another site or device in the breadcrumb loads the page anew
            SiteView::Devices => html! {<Devices key={site} {site}/>},
            SiteView::Device { id, view } => view.content(id),
        }
    }
}
impl DeviceView {
    pub fn content(self, id: u32) -> Html {
        match self {
            Self::Show => html! {<ShowDevice key={id} {id}/>},
        }
    }
}
