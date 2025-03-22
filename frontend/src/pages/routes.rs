use patternfly_yew::prelude::{Nav, NavRouterItem};
use yew::{Html, function_component, html};
use yew_nested_router::Target;

#[derive(Debug, Default, Clone, PartialEq, Eq, Target)]
pub enum AppRoute {
    NotFound,
    #[default]
    Devices,
}

impl AppRoute {
    pub fn content(self) -> Html {
        match self {
            AppRoute::Devices => html! {<h1>{"Home"}</h1>},
            AppRoute::NotFound => html! {<h1>{"Not Found"}</h1>},
        }
    }
}

#[function_component(Sidebar)]
pub fn sidebar() -> Html {
    html! {
        <Nav>
            <NavRouterItem<AppRoute> to={AppRoute::Devices}>{"Devices"}</NavRouterItem<AppRoute>>
        </Nav>
    }
}
