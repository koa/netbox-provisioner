//! Scope: The routes of the UI; not what they show (`app`, `pages`).
//!
//! The routes of the UI: what the breadcrumb and the links lead to (`app` renders each one's
//! page).
use yew_nested_router::Target;

/// `/sites`, `/network-map` (every site), `/rollout` (updates across sites), `/site/<site>`
/// (its devices), `/site/<site>/map`, `/site/<site>/device/<device>/show`
#[derive(Debug, Clone, Default, PartialEq, Eq, Target)]
pub enum AppRoute {
    NotFound,
    #[default]
    Sites,
    #[target(rename = "network-map")]
    NetworkMap,
    Rollout,
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
    Map,
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
}
