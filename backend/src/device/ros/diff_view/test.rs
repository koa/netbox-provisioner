//! Scope: Tests of `device::ros::diff_view`; nothing else.
use super::*;

fn pair(key: &'static [u8], value: &'static [u8]) -> KeyValuePair<'static> {
    KeyValuePair {
        key,
        value: Cow::Borrowed(value),
    }
}

fn mutation(
    resource: &'static [u8],
    operation: ResourceMutationOperation<'static>,
    fields: &[KeyValuePair<'static>],
    before: &[KeyValuePair<'static>],
) -> ResourceMutation<'static> {
    ResourceMutation {
        resource,
        operation,
        fields: fields.into(),
        before: before.into(),
        depends: Box::new([]),
        provides: Box::new([]),
        ordering: Default::default(),
    }
}

/// A rename, an unset field, a removal and an add, as the script runs them.
fn mutations() -> [ResourceMutation<'static>; 4] {
    [
        mutation(
            b"interface/ethernet",
            ResourceMutationOperation::UpdateByKey(pair(b"name", b"e05")),
            &[pair(b"name", b"e05-test"), pair(b"!comment", b"")],
            &[pair(b"name", b"e05"), pair(b"comment", b"by hand")],
        ),
        mutation(
            b"ip/address",
            ResourceMutationOperation::RemoveByKey(pair(b"address", b"10.0.0.1/24")),
            &[],
            &[pair(b"address", b"10.0.0.1/24"), pair(b"interface", b"e05")],
        ),
        mutation(
            b"interface/bridge/port",
            ResourceMutationOperation::Add,
            &[pair(b"bridge", b"bridge"), pair(b"interface", b"e05-test")],
            &[],
        ),
        mutation(
            b"system/identity",
            ResourceMutationOperation::UpdateSingle,
            &[pair(b"name", b"new")],
            &[pair(b"name", b"old")],
        ),
    ]
}

/// Each entry with its fields before and after; an unset field has no value.
#[test]
fn entries_name_the_values_before_and_after() {
    let entries = entry_changes(&mutations());
    let ethernet = &entries[0];
    assert_eq!(ethernet.kind, EntryChangeKind::Set);
    assert_eq!(ethernet.entry.as_deref(), Some("name=e05"));
    assert_eq!(
        ethernet.fields.as_ref(),
        [
            DiffField {
                field: "name".into(),
                before: Some("e05".into()),
                after: Some("e05-test".into()),
            },
            DiffField {
                field: "comment".into(),
                before: Some("by hand".into()),
                after: None,
            },
        ]
    );
    assert_eq!(entries[1].kind, EntryChangeKind::Remove);
    assert_eq!(entries[1].fields[1].before.as_deref(), Some("e05"));
    assert_eq!(entries[2].kind, EntryChangeKind::Add);
    assert_eq!(entries[2].entry, None);
    assert_eq!(entries[3].fields[0].before.as_deref(), Some("old"));
}

/// The way back runs the other way: the add removed, the removal added again, the renamed
/// port found by its new name and given its old name and comment back.
#[test]
fn the_script_back_undoes_each_in_reverse() {
    let script = revert_script(&mutations()).unwrap();
    assert_eq!(
        script.as_ref(),
        "/system/identity\n\
         set name=old \n\
         /interface/bridge/port\n\
         remove [find bridge=bridge interface=e05-test dynamic!=yes]\n\
         /ip/address\n\
         add address=\"10.0.0.1/24\" interface=e05 \n\
         /interface/ethernet\n\
         set [ find name=e05-test] name=e05 comment=\"by hand\" \n"
    );
}
