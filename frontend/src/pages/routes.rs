use crate::pages::devices::Devices;
use crate::pages::devices::show::ShowDevice;
use patternfly_yew::prelude::{Nav, NavRouterItem};
use yew::{Html, function_component, html};
use yew_nested_router::Target;

#[derive(Debug, Clone, PartialEq, Eq, Target)]
pub enum AppRoute {
    NotFound,
    Devices(RouteDevices),
}

impl Default for AppRoute {
    fn default() -> Self {
        Self::Devices(RouteDevices::List)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Target)]
pub enum RouteDevices {
    #[target(index)]
    List,
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
    pub fn content(self) -> Html {
        match self {
            AppRoute::Devices(d) => d.content(),
            AppRoute::NotFound => html! {<h1>{"Not Found"}</h1>},
        }
    }
}

impl RouteDevices {
    pub fn content(self) -> Html {
        match self {
            RouteDevices::List => html! {<h1><Devices/></h1>},
            RouteDevices::Device { id, view } => view.content(id),
        }
    }
}
impl DeviceView {
    pub fn content(self, id: u32) -> Html {
        match self {
            Self::Show => html! {<ShowDevice {id}/>},
        }
    }
}

#[function_component(Sidebar)]
pub fn sidebar() -> Html {
    html! {
        <Nav>
            <NavRouterItem<AppRoute> to={AppRoute::Devices(RouteDevices::List)}>{"Devices"}</NavRouterItem<AppRoute>>
        </Nav>
    }
}
