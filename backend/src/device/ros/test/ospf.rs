//! Scope: Tests of the base config's OSPF; nothing else.
use crate::device::ros::{
    BaseDeviceDataCurrent, BaseDeviceDataTarget, cfg::in_script, ospf::replace_inactive_templates,
    test::empty_current,
};
use mikrotik_model::{
    ascii::AsciiString,
    model::RoutingOspfInterfaceTemplateById,
    resource::{ResourceMutation, ResourceMutationOperation},
};

/// The operations of `mutations` on `/routing/ospf/interface-template` the script has, as `add`
/// or the key removed.
fn template_changes(mutations: &[ResourceMutation]) -> Vec<String> {
    mutations
        .iter()
        .filter(|m| m.resource.ends_with(b"routing/ospf/interface-template") && in_script(m))
        .map(|m| match &m.operation {
            ResourceMutationOperation::Add => "add".to_string(),
            ResourceMutationOperation::RemoveByKey(key) => {
                format!("remove {}", String::from_utf8_lossy(&key.value))
            }
            other => format!(
                "{other:?} {:?}",
                m.fields
                    .iter()
                    .map(|f| format!(
                        "{}={}",
                        String::from_utf8_lossy(f.key),
                        String::from_utf8_lossy(&f.value)
                    ))
                    .collect::<Vec<_>>()
            ),
        })
        .collect()
}

/// A template RouterOS reports inactive looks like the target's (the model completes its few
/// fields with RouterOS's defaults), so only its `inactive` makes the diff replace it: removed
/// by its `.id`, the target's added anew (berghof-haus-stall, 7.24.6: no OSPFv2 interface).
#[test]
fn an_inactive_template_is_replaced() {
    let mut target = BaseDeviceDataTarget::new(b"RB750Gr3").unwrap();
    let template = target
        .ospf_interface
        .entry((b"backbone-v2".into(),))
        .or_default();
    template.interfaces = [AsciiString::from(b"bridge-bridge".as_slice())].into();
    template.area = b"backbone-v2".as_slice().into();
    template.priority = 0;
    let template = template.clone();
    let mut current: BaseDeviceDataCurrent = empty_current(&target);
    current.ospf_interface = Box::new([RoutingOspfInterfaceTemplateById {
        id: b"*1".as_slice().into(),
        data: template,
    }]);
    let changes = |current: &BaseDeviceDataCurrent| {
        template_changes(&target.generate_mutations(current).unwrap())
    };
    let active = changes(&current);
    assert!(active.is_empty(), "active, it is the target's: {active:?}");

    let removals = replace_inactive_templates(&mut current, &[b"*1".as_slice().into()]);
    assert_eq!(template_changes(&removals), ["remove *1"]);
    assert_eq!(changes(&current), ["add"]);
}
