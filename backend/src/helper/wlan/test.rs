//! Scope: Tests of `helper::wlan`; nothing else.
use super::{WlanInput, plan};
use crate::{
    Error,
    helper::PlanStep,
    topology::{WlanAuth, test::example_topology},
};

fn input(wlan: u32, ssid: Option<&str>, psk: Option<&str>) -> WlanInput {
    WlanInput {
        wlan,
        ssid: ssid.map(String::from),
        psk: psk.map(String::from),
        enabled: None,
    }
}

/// The export's WLAN „koa“ (1) in the group „Berg“, with „berg“ (2) beside it.
const KOA: u32 = 1;

/// A new key and SSID are one step for the group's controller; the preview has both, and
/// neither key is in the plan's debug output (the fingerprint, the log).
#[test]
fn a_new_key_and_ssid_are_the_controllers_plan() {
    let topology = example_topology();
    let wlan = super::wlan(&topology, KOA).expect("koa");
    let controller = wlan
        .wlan_group()
        .and_then(|group| group.controller())
        .expect("a controller");
    let plan = plan(
        &topology,
        &input(KOA, Some("koa-neu"), Some("ein langes Passwort")),
    )
    .expect("a plan");
    assert_eq!(plan.device().id(), controller.id());
    let [PlanStep::UpdateWirelessLan(step)] = &plan.steps[..] else {
        panic!("{:?}", plan.steps);
    };
    assert_eq!(step.ssid.as_deref(), Some("koa-neu"));
    assert_eq!(&*step.ssid_before, "koa");
    assert!(step.psk_set);
    let debug = format!("{:?}", plan.steps);
    assert!(!debug.contains("ein langes Passwort"), "{debug}");
    assert!(!debug.contains(&*step.psk_before), "{debug}");
    let preview = plan.preview_topology().expect("a preview");
    let after = super::wlan(&preview, KOA).expect("koa after");
    assert_eq!(after.ssid(), Some("koa-neu"));
    assert!(matches!(after.auth(), Some(WlanAuth::Wpa(wpa)) if wpa.psk() == "ein langes Passwort"));
}

/// What a resident can't set: a short key, an SSID of another WLAN of the group, one too
/// long, the same values (nothing to change), a key for an open WLAN.
#[test]
fn bad_values_are_refused() {
    let topology = example_topology();
    let refused = |input: WlanInput| plan(&topology, &input).err().expect("refused");
    assert!(matches!(
        refused(input(KOA, None, Some("kurz"))),
        Error::PskInvalid
    ));
    assert!(matches!(
        refused(input(KOA, None, Some("ein langes Pässwort"))),
        Error::PskInvalid
    ));
    assert!(matches!(
        refused(input(KOA, Some("berg"), None)),
        Error::SsidTaken(ssid) if &*ssid == "berg"
    ));
    assert!(matches!(
        refused(input(KOA, Some(&"x".repeat(33)), None)),
        Error::SsidInvalid
    ));
    assert!(matches!(
        refused(input(KOA, Some(" koa"), None)),
        Error::SsidInvalid
    ));
    assert!(matches!(
        refused(input(KOA, Some("koa"), None)),
        Error::NothingToChange
    ));
    let open = (1..100)
        .find(|id| {
            super::wlan(&topology, *id)
                .is_ok_and(|wlan| matches!(wlan.auth(), Some(WlanAuth::Open(_))))
        })
        .expect("an open WLAN in the export");
    assert!(matches!(
        refused(input(open, None, Some("ein langes Passwort"))),
        Error::WlanNotPersonal(_)
    ));
    assert!(matches!(
        refused(input(9999, None, Some("ein langes Passwort"))),
        Error::UnknownWlan(9999)
    ));
}

/// Switching off is a step of its own (NetBox `status` `disabled`): on the preview the WLAN is
/// still there, off, and its group sends it no more; off again is nothing to change, and on
/// brings it back.
#[test]
fn a_wlan_is_switched_off_and_on() {
    let topology = example_topology();
    let switch = |enabled| WlanInput {
        enabled: Some(enabled),
        ..input(KOA, None, None)
    };
    let plan = plan(&topology, &switch(false)).expect("a plan");
    let [PlanStep::UpdateWirelessLan(step)] = &plan.steps[..] else {
        panic!("{:?}", plan.steps);
    };
    assert_eq!(step.enabled, Some(false));
    assert_eq!(step.ssid, None);
    assert!(!step.psk_set);
    let off = plan.preview_topology().expect("a preview");
    let wlan = super::wlan(&off, KOA).expect("koa when off");
    assert!(!wlan.enabled());
    let group = wlan.wlan_group().expect("its group");
    assert!(group.wlan().all(|other| other.ssid() != Some("koa")));
    assert!(group.all_wlans().any(|other| other.ssid() == Some("koa")));
    assert!(matches!(
        super::plan(&off, &switch(false)).err(),
        Some(Error::NothingToChange)
    ));
    let on = super::plan(&off, &switch(true)).expect("a plan to switch on");
    let on = on.preview_topology().expect("a preview");
    assert!(super::wlan(&on, KOA).expect("koa").enabled());
}
