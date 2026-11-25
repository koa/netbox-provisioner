//! Scope: How a resident's port or socket is named in its row and in texts; nothing else.
use crate::graphql::resident::residents::{PortKind, ResidentPort};
use i18n::resident as t;

/// A port row's name: its label, a socket by its wall and place, or the port's name.
pub fn title(port: &ResidentPort) -> String {
    match &port.label {
        Some(label) => label.clone(),
        // a socket's device is the wall it is in, named after the room
        None if port.kind == PortKind::Socket => t::socket_name(&port.device.name, &port.name),
        None => port.name.clone(),
    }
}

/// A port by itself and its device: „Port ether3 an lab-np2“, „Port ether2 von lab-wap“.
pub fn sub(port: &ResidentPort) -> String {
    match port.on_own_device {
        true => t::own_port(&port.name, &port.device.name),
        false => t::port_of(&port.name, &port.device.name),
    }
}

/// A port in a text: a socket by its name, another port by itself and its device („Port ether3
/// an lab-np2“), as its label names the device at it.
pub fn place(port: &ResidentPort) -> String {
    match port.kind {
        PortKind::Socket => title(port),
        _ => sub(port),
    }
}

#[cfg(test)]
mod test {
    use super::place;
    use crate::graphql::resident::residents::{Named, PortKind, ResidentPort};

    fn named(id: i32, name: &str) -> Named {
        Named {
            id,
            name: name.into(),
        }
    }

    /// A port in a text is the socket by its name, another port by itself and its switch: its
    /// label names the device at it.
    #[test]
    fn a_port_in_a_text() {
        let mut port = ResidentPort {
            id: 13,
            kind: PortKind::Socket,
            name: "1".into(),
            label: Some("Wohnzimmer".into()),
            device: named(50, "wohnung-meier"),
            site: None,
            location: Vec::new(),
            on_own_device: false,
            network: None,
            foreign_network: false,
            far_ends: Vec::new(),
            routeros: true,
        };
        assert_eq!(place(&port), "Wohnzimmer");
        port.kind = PortKind::Interface;
        port.name = "ether3".into();
        port.label = Some("lab-wap".into());
        port.device = named(11, "lab-np2");
        assert_eq!(place(&port), "Port ether3 an lab-np2");
    }
}
