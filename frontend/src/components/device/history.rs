//! The device page's „Verlauf“: the provisioner's entries in the device's journal in NetBox
//! (`DeviceAccess.journal`, `docs/protocol.md`), newest first, each linking to NetBox. Read again
//! whenever a job of the page ends (`refresh`).
use crate::{
    error::FrontendError,
    graphql::{
        authenticated::{
            history::{
                DeviceJournal, DeviceJournalVariables, JournalEntry as Entry, JournalLeft,
                JournalOutcome,
            },
            types::JobKind,
        },
        request,
    },
    i18n::{
        job::{self as jobs, phase},
        journal::{self as t, Left, Outcome, history as h},
    },
};
use cynic::QueryBuilder;
use patternfly_yew::prelude::{Card, CardBody, CardHeader, CardTitle};
use yew::{Component, Context, Html, Properties, html, platform::spawn_local};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct HistoryProps {
    pub id: u32,
    /// changes when a job ended: read again
    pub refresh: u32,
}

pub struct History {
    entries: Option<Vec<Entry>>,
    error: Option<FrontendError>,
}

pub enum HistoryMsg {
    Loaded(Result<DeviceJournal, FrontendError>),
}

fn fetch(ctx: &Context<History>) {
    let scope = ctx.link().clone();
    let device = ctx.props().id as i32;
    spawn_local(async move {
        let journal = DeviceJournal::build(DeviceJournalVariables { device });
        let loaded = request(scope.clone(), journal).await;
        scope.send_message(HistoryMsg::Loaded(loaded));
    });
}

impl Component for History {
    type Message = HistoryMsg;
    type Properties = HistoryProps;

    fn create(ctx: &Context<Self>) -> Self {
        fetch(ctx);
        Self {
            entries: None,
            error: None,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            HistoryMsg::Loaded(Ok(data)) => {
                self.entries = data.topology.device_by_id.map(|device| device.journal);
                self.error = None;
            }
            HistoryMsg::Loaded(Err(error)) => self.error = Some(error),
        }
        true
    }

    fn changed(&mut self, ctx: &Context<Self>, old: &Self::Properties) -> bool {
        let props = ctx.props();
        if props.id != old.id || props.refresh != old.refresh {
            fetch(ctx);
        }
        false
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        let error = self.error.as_ref().map(FrontendError::to_html);
        let body = match &self.entries {
            None => html! {},
            Some(entries) if entries.is_empty() => html! {<p>{h::NONE}</p>},
            Some(entries) => html! {
                <ul class="device-history">{ for entries.iter().map(entry) }</ul>
            },
        };
        html! {
            <Card class="device-history-card">
                <CardHeader><CardTitle>{h::TITLE}</CardTitle></CardHeader>
                <CardBody>{error}{body}</CardBody>
            </Card>
        }
    }
}

fn entry(entry: &Entry) -> Html {
    // NetBox gives `2026-10-25T13:04:30.123456+00:00`, in its own time zone
    let created = entry
        .created
        .get(..16)
        .unwrap_or(&entry.created)
        .replace('T', " ");
    let link = html! {
        <div><a href={entry.url.clone()} target="_blank" rel="noopener">{h::IN_NETBOX}</a></div>
    };
    if let Some(seen) = &entry.observation {
        let lefts = |list: &[JournalLeft]| list.iter().filter_map(left).collect::<Vec<_>>();
        let (fired, found, removed) =
            (lefts(&seen.fired), lefts(&seen.found), lefts(&seen.removed));
        let class = match (fired.is_empty(), found.is_empty()) {
            (false, _) => "failed",
            (true, false) => "warning",
            (true, true) => "",
        };
        return html! {
            <li class={class}>
                <strong>{h::OBSERVED}</strong>{" · "}{created}
                if !fired.is_empty() {
                    <div>{t::fired(&fired)}</div>
                }
                if !found.is_empty() {
                    <div>{t::found(&found)}</div>
                }
                if !removed.is_empty() {
                    <div>{t::removed(&removed)}</div>
                }
                {link}
            </li>
        };
    }
    let outcome = outcome(&entry.outcome);
    let class = match outcome {
        Outcome::Running => "running",
        Outcome::Done => "done",
        Outcome::Incomplete | Outcome::Cancelled | Outcome::Interrupted => "warning",
        Outcome::Failed => "failed",
    };
    let state = entry
        .state
        .as_deref()
        .filter(|_| outcome != Outcome::Done)
        .map(|state| {
            t::state(
                phase::of(state).unwrap_or(state),
                outcome == Outcome::Running,
            )
        });
    let left: Vec<Left> = entry.left.iter().filter_map(left).collect();
    html! {
        <li class={class}>
            <strong>{entry.kind.as_ref().map_or("", kind)}</strong>{" "}{t::how(outcome)}
            {" · "}{created}
            if let Some(by) = &entry.by {
                {" · "}{h::by(by)}
            }
            if let Some(state) = state {
                <div>{state}</div>
            }
            if !left.is_empty() {
                <div>{t::left(&left)}</div>
            }
            {link}
        </li>
    }
}

fn outcome(outcome: &JournalOutcome) -> Outcome {
    match outcome {
        JournalOutcome::Running | JournalOutcome::Other => Outcome::Running,
        JournalOutcome::Done => Outcome::Done,
        JournalOutcome::Incomplete => Outcome::Incomplete,
        JournalOutcome::Failed => Outcome::Failed,
        JournalOutcome::Cancelled => Outcome::Cancelled,
        JournalOutcome::Interrupted => Outcome::Interrupted,
    }
}

fn left(left: &JournalLeft) -> Option<Left> {
    match left {
        JournalLeft::ApplyFallback => Some(Left::ApplyFallback),
        JournalLeft::UpdateFallback => Some(Left::UpdateFallback),
        JournalLeft::WatchdogSafetyNet => Some(Left::WatchdogSafetyNet),
        JournalLeft::Other => None,
    }
}

fn kind(kind: &JobKind) -> &str {
    match kind {
        JobKind::Update => jobs::UPDATE,
        JobKind::Downgrade => jobs::DOWNGRADE,
        JobKind::Cleanup => jobs::CLEANUP,
        JobKind::Apply => jobs::APPLY,
        JobKind::Bootstrap => jobs::BOOTSTRAP,
        JobKind::Reboot => jobs::REBOOT,
        JobKind::PowerCycle => jobs::POWER_CYCLE,
        JobKind::DeviceMode => jobs::DEVICE_MODE,
        JobKind::Repartition => jobs::REPARTITION,
        JobKind::BootPartition => jobs::BOOT_PARTITION,
        JobKind::NetboxWrite => jobs::NETBOX_WRITE,
        JobKind::NetboxUndo => jobs::NETBOX_UNDO,
        JobKind::SiteDiff => jobs::SITE_DIFF,
        JobKind::Other(other) => other,
    }
}
