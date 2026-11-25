//! Scope: The device page's card „Gerät“ around RouterOS, the device mode and the partitions;
//! not what it holds (`routeros`, `device_mode`, `partitions`).
use crate::i18n::device as t;
use patternfly_yew::prelude::{Card, CardBody, CardHeader, CardTitle};
use yew::{Html, Properties, function_component, html};

/// The parts read of the device, each groups of the description list; `None` where it couldn't
/// be read or doesn't have it.
#[derive(Debug, Clone, PartialEq, Properties)]
pub struct StateCardProps {
    pub routeros: Option<Html>,
    pub device_mode: Option<Html>,
    pub partitions: Option<Html>,
}

/// „Gerät“; nothing where no part could be read.
#[function_component(StateCard)]
pub fn state_card(props: &StateCardProps) -> Html {
    if props.routeros.is_none() && props.device_mode.is_none() && props.partitions.is_none() {
        return Html::default();
    }
    html! {
        <Card class="device-state">
            <CardHeader><CardTitle>{t::DEVICE}</CardTitle></CardHeader>
            <CardBody>
                <dl class="device-overview">
                    { props.routeros.clone() }
                    { props.device_mode.clone() }
                    { props.partitions.clone() }
                </dl>
            </CardBody>
        </Card>
    }
}
