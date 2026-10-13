//! The breadcrumb, after cable-editor: every item a dropdown leading to its siblings (another
//! site, another device of the site).
use crate::{
    error::FrontendError,
    graphql::{
        authenticated::{ListSites, list_sites},
        query_authenticated,
    },
    pages::routes::{AppRoute, SiteView},
};
use patternfly_yew::prelude::{Icon, MenuToggleVariant, Spinner};
use popup::{MenuGroup, MenuLinkItem, PopupMenu};
use yew::{
    AttrValue, Component, Context, Html, Properties, function_component, html,
    html::ChildrenProps, platform::spawn_local,
};

pub mod popup;

#[derive(Properties, PartialEq)]
pub struct MenuDropdownProps {
    pub title: AttrValue,
    pub entries: Box<[MenuEntry]>,
}

#[derive(PartialEq, Clone)]
pub struct MenuEntry {
    /// Marked as selected also when `target` is not the current page: the site of the device
    /// shown
    pub selected: bool,
    pub text: AttrValue,
    pub target: AppRoute,
}

/// One item of the breadcrumb, with the divider before it (PatternFly hides the first one). The
/// list is rendered by `Navigation` itself, as patternfly-yew's `Breadcrumb` takes only its own
/// items.
#[function_component]
pub fn BreadcrumbItem(props: &ChildrenProps) -> Html {
    html! {
        <li class="pf-v6-c-breadcrumb__item">
            <span class="pf-v6-c-breadcrumb__item-divider">{Icon::AngleRight}</span>
            {props.children.clone()}
        </li>
    }
}

/// Dropdown for a breadcrumb item: a `PopupMenu` of router links, the current page marked.
#[function_component]
pub fn MenuDropdown(props: &MenuDropdownProps) -> Html {
    let links = props
        .entries
        .iter()
        .map(|entry| {
            html! {
                <MenuLinkItem
                    key={entry.text.as_str()}
                    to={entry.target.clone()}
                    selected={entry.selected}
                >
                    {entry.text.clone()}
                </MenuLinkItem>
            }
        })
        .collect::<Html>();
    html! {
        <PopupMenu variant={MenuToggleVariant::Plain} text={html!(props.title.clone())}>
            <MenuGroup>{links}</MenuGroup>
        </PopupMenu>
    }
}

#[derive(Properties, PartialEq)]
pub struct NavigationProps {
    #[prop_or_default]
    pub site: Option<u32>,
    #[prop_or_default]
    pub device: Option<u32>,
}

/// The breadcrumb of a page: the site, and on a site's pages its device.
pub struct Navigation {
    sites: Option<Result<Vec<list_sites::ListSitesTopologyAllSites>, FrontendError>>,
}

pub enum NavigationMsg {
    Sites(Result<Vec<list_sites::ListSitesTopologyAllSites>, FrontendError>),
}

impl Component for Navigation {
    type Message = NavigationMsg;
    type Properties = NavigationProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self { sites: None }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            NavigationMsg::Sites(sites) => self.sites = Some(sites),
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let items = match &self.sites {
            None => html!(<BreadcrumbItem><Spinner/></BreadcrumbItem>),
            Some(Err(error)) => html! {
                <BreadcrumbItem>
                    <span class="menu-error" title={error.to_string()}>
                        {Icon::ExclamationCircle}{" Nicht geladen"}
                    </span>
                </BreadcrumbItem>
            },
            Some(Ok(sites)) => self.items(ctx.props(), sites),
        };
        html! {
            <nav class="pf-v6-c-breadcrumb" aria-label="breadcrumb">
                <ol class="pf-v6-c-breadcrumb__list" role="list">{items}</ol>
            </nav>
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
                scope.send_message(NavigationMsg::Sites(sites));
            });
        }
    }
}

impl Navigation {
    fn items(&self, props: &NavigationProps, sites: &[list_sites::ListSitesTopologyAllSites]) -> Html {
        let site = props
            .site
            .and_then(|id| sites.iter().find(|site| site.id as u32 == id));
        let overview = MenuEntry {
            selected: false,
            text: "Übersicht".into(),
            target: AppRoute::Sites,
        };
        let site_entries = Some(overview)
            .into_iter()
            .chain(sites.iter().map(|entry| MenuEntry {
                selected: site.is_some_and(|site| site.id == entry.id),
                text: entry.name.clone().into(),
                target: AppRoute::Site {
                    id: entry.id as u32,
                    view: SiteView::Devices,
                },
            }))
            .collect::<Box<[_]>>();
        let site_title = match (props.site, site) {
            (_, Some(site)) => format!("Site: {}", site.name),
            (Some(_), None) => "Site: unbekannt".to_string(),
            (None, None) => "Site: -".to_string(),
        };
        let site_menu = html! {
            <BreadcrumbItem>
                <MenuDropdown title={site_title} entries={site_entries}/>
            </BreadcrumbItem>
        };
        let Some(site) = site else {
            return site_menu;
        };
        let device = props
            .device
            .and_then(|id| site.devices.iter().find(|device| device.id as u32 == id));
        let device_entries = Some(MenuEntry {
            selected: false,
            text: "Alle Geräte".into(),
            target: AppRoute::Site {
                id: site.id as u32,
                view: SiteView::Devices,
            },
        })
        .into_iter()
        .chain(site.devices.iter().map(|entry| MenuEntry {
            selected: false,
            text: entry.name.clone().into(),
            target: AppRoute::device(site.id as u32, entry.id as u32),
        }))
        .collect::<Box<[_]>>();
        let device_title = match (props.device, device) {
            (_, Some(device)) => format!("Gerät: {}", device.name),
            (Some(_), None) => "Gerät: unbekannt".to_string(),
            (None, None) => "Gerät: -".to_string(),
        };
        html! {
            <>
                {site_menu}
                <BreadcrumbItem>
                    <MenuDropdown title={device_title} entries={device_entries}/>
                </BreadcrumbItem>
            </>
        }
    }
}
