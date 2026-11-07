//! Scope: The partitions on the device page, booting from one and splitting a single one;
//! nothing else.
use crate::{
    components::widgets::confirm::Confirm, graphql::authenticated::device::Partitions as Parts,
    i18n::device as t,
};
use yew::{Callback, Component, Context, Html, Properties, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct PartitionsProps {
    pub partitions: Parts,
    /// a job runs, or writing is locked
    pub disabled: bool,
    /// „In zwei Partitionen aufteilen“, confirmed
    pub on_split: Callback<()>,
    /// „Von partN starten“, confirmed: the partition
    pub on_boot: Callback<String>,
}

/// The partitions as a `<dt>`/`<dd>` pair, each with a RouterOS to boot from.
pub struct Partitions {
    /// „In zwei Partitionen aufteilen“ was clicked, waiting for the confirmation
    confirm_split: bool,
    /// the partition to boot from, asked before
    confirm_boot: Option<String>,
}

pub enum PartitionsMsg {
    ConfirmSplit(bool),
    Split,
    ConfirmBoot(Option<String>),
    Boot(String),
}

impl Component for Partitions {
    type Message = PartitionsMsg;
    type Properties = PartitionsProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            confirm_split: false,
            confirm_boot: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            PartitionsMsg::ConfirmSplit(confirm) => self.confirm_split = confirm,
            PartitionsMsg::Split => {
                self.confirm_split = false;
                ctx.props().on_split.emit(());
            }
            PartitionsMsg::ConfirmBoot(partition) => self.confirm_boot = partition,
            PartitionsMsg::Boot(partition) => {
                self.confirm_boot = None;
                ctx.props().on_boot.emit(partition);
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let split = Confirm {
            asking: self.confirm_split,
            question: t::PARTITIONS_CONFIRM.into(),
            now: t::PARTITIONS_NOW,
            ask: t::PARTITIONS_SPLIT.into(),
            disabled: props.disabled,
        };
        html! {<>
            <dt>{t::PARTITIONS}</dt><dd>
                { for props.partitions.partitions.iter().map(|p| {
                    // a RouterOS to boot, e.g. the one from before an update
                    let bootable = !p.running && !p.version.is_empty();
                    let boot = bootable.then(|| {
                        let (now, ask) = (p.name.clone(), p.name.clone());
                        Confirm {
                            asking: self.confirm_boot.as_ref() == Some(&p.name),
                            question: t::PARTITION_BOOT_CONFIRM.into(),
                            now: t::PARTITION_BOOT_NOW,
                            ask: t::partition_boot(&p.name),
                            disabled: props.disabled,
                        }
                        .view(
                            ctx.link().callback(move |_| PartitionsMsg::Boot(now.clone())),
                            ctx.link().callback(move |asking: bool| {
                                PartitionsMsg::ConfirmBoot(asking.then(|| ask.clone()))
                            }),
                        )
                    });
                    html!(
                    <div>
                        { t::partition(&p.name, &p.version, p.size_mib, p.active, p.running) }
                        if let Some(boot) = boot {
                            {" "}{boot}
                        }
                    </div>
                )})}
                if props.partitions.can_split {
                    <div class="partitions-split">
                        { split.view(
                            ctx.link().callback(|_| PartitionsMsg::Split),
                            ctx.link().callback(PartitionsMsg::ConfirmSplit),
                        ) }
                    </div>
                }
            </dd>
        </>}
    }
}
