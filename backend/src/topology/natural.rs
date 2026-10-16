//! NetBox' order of names (`utilities/ordering.py`), so lists read as in NetBox: `naturalize`
//! for devices, `naturalize_interface` for interfaces (`ether2` before `ether10`, all `ether`
//! before `sfp-sfpplus`, a slot `1/` first).
use regex::Regex;
use std::sync::LazyLock;

/// NetBox' `INTERFACE_NAME_REGEX`.
static INTERFACE_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<type>[^\d\.:]+)?((?P<slot>\d+)/)?((?P<subslot>\d+)/)?((?P<position>\d+)/)?((?P<subposition>\d+)/)?((?P<id>\d+))?(:(?P<channel>\d+))?(\.(?P<vc>\d+))?(?P<remainder>.*)$",
    )
    .expect("valid regex")
});

/// The key NetBox sorts a name by: every number padded to 8 digits.
pub fn naturalize(value: &str) -> String {
    let mut output = String::new();
    let mut digits = String::new();
    for char in value.chars() {
        if char.is_ascii_digit() {
            digits.push(char);
        } else {
            if !digits.is_empty() {
                output.push_str(&format!("{digits:0>8}"));
                digits.clear();
            }
            output.push(char);
        }
    }
    if !digits.is_empty() {
        output.push_str(&format!("{digits:0>8}"));
    }
    output
}

/// The key NetBox sorts an interface name by: slot, subslot, position and subposition
/// (`9999` where missing), the type, then id, channel and virtual circuit, the rest
/// naturalized.
pub fn naturalize_interface(value: &str) -> String {
    let Some(parts) = INTERFACE_NAME.captures(value) else {
        return value.to_string();
    };
    let mut output = String::new();
    for name in ["slot", "subslot", "position", "subposition"] {
        match parts.name(name) {
            Some(part) => output.push_str(&format!("{:0>4}", part.as_str())),
            None => output.push_str("9999"),
        }
    }
    if let Some(kind) = parts.name("type") {
        output.push_str(kind.as_str());
    }
    for name in ["id", "channel", "vc"] {
        match parts.name(name) {
            Some(part) => output.push_str(&format!("{:0>6}", part.as_str())),
            None => output.push_str("......"),
        }
    }
    if let Some(remainder) = parts.name("remainder") {
        output.push_str(&naturalize(remainder.as_str()));
    }
    output
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn interfaces_in_netbox_order() {
        let mut names = [
            "sfp-sfpplus2", "ether10", "lo", "ether2", "sfp-sfpplus1", "ether1", "wlan1",
            "1/2", "ether1.100",
        ];
        names.sort_by_key(|name| naturalize_interface(name));
        assert_eq!(
            names,
            [
                "1/2", "ether1", "ether1.100", "ether2", "ether10", "lo", "sfp-sfpplus1",
                "sfp-sfpplus2", "wlan1"
            ]
        );
    }

    #[test]
    fn devices_in_netbox_order() {
        let mut names = ["ap-10", "ap-2", "ap-1og", "router"];
        names.sort_by_key(|name| naturalize(name));
        assert_eq!(names, ["ap-1og", "ap-2", "ap-10", "router"]);
    }
}
