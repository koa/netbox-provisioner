//! Scope: What a helper's write says in NetBox's changelog and the descriptions it gives the
//! objects it creates; not the helpers' forms (`helper`) nor the journal (`journal`).
//!
//! A helper's plan (`backend::helper::NetboxPlan`) knows what it does as a [`Plan`] with its
//! values; NetBox's changelog gets it worded here, with the user ([`message`]).

use std::fmt::{self, Display, Formatter};

/// A port, by its device's name and its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    pub device: Box<str>,
    pub name: Box<str>,
}

impl Display for Port {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.device, self.name)
    }
}

/// What a helper's plan does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Loopback {
        device: Box<str>,
    },
    /// a direct link between two routers
    Connection {
        a: Port,
        b: Port,
    },
    /// the link at a port taken out of NetBox
    ConnectionCleared {
        port: Port,
    },
    /// the links in the way cleared, then `connection`
    CableTakeover {
        connection: Box<Plan>,
    },
    /// a device's port linked to a router's
    Uplink {
        device: Port,
        router: Port,
    },
    /// a customer network on a port, with its label
    CustomerNetPort {
        port: Port,
        label: Option<Box<str>>,
    },
    /// a customer network on a VLAN
    CustomerNetVlan {
        device: Box<str>,
        vlan: Box<str>,
        vid: u16,
    },
    CustomerDisconnect {
        port: Port,
    },
    BridgeLeave {
        port: Port,
        bridge: Box<str>,
    },
    AddressTakeover {
        address: Box<str>,
        port: Port,
    },
    LeaseTakeover {
        name: Box<str>,
        address: Box<str>,
        router: Box<str>,
    },
    NewDevice {
        name: Box<str>,
        port: Port,
    },
    Flags {
        device: Box<str>,
    },
    Wlan {
        ssid: Box<str>,
    },
    DeviceRoom {
        device: Box<str>,
    },
    /// a port's VLAN, none to take it off
    PortVlan {
        port: Port,
        vid: Option<u16>,
    },
    /// a port feeding another
    Poe {
        pse: Port,
        pd: Port,
    },
    /// a port feeding, its device not in NetBox yet
    PoeFeeding {
        port: Port,
    },
    /// a port's PoE mode taken off, and its counterpart's
    NoPoe {
        port: Port,
    },
    RadioBridge {
        ap: Box<str>,
        station: Box<str>,
    },
    RadioDisconnect {
        devices: Box<[Box<str>]>,
    },
}

/// What a plan does, in a few words.
pub fn plan(plan: &Plan) -> String {
    match plan {
        Plan::Loopback { device } => format!("Loopback {device}"),
        Plan::Connection { a, b } => format!("Verbindung {a} – {b}"),
        Plan::ConnectionCleared { port } => format!("Verbindung abräumen {port}"),
        Plan::CableTakeover { connection } => {
            format!("Kabel übernehmen: {}", self::plan(connection))
        }
        Plan::Uplink { device, router } => format!("Anschluss {device} an {router}"),
        Plan::CustomerNetPort { port, label } => match label {
            Some(label) => format!("Kunden-Netz {port} ({label})"),
            None => format!("Kunden-Netz {port}"),
        },
        Plan::CustomerNetVlan { device, vlan, vid } => {
            format!("Kunden-Netz {device} VLAN {vlan} ({vid})")
        }
        Plan::CustomerDisconnect { port } => format!("Kunden-Anschluss abräumen {port}"),
        Plan::BridgeLeave { port, bridge } => format!("Aus Bridge lösen {port} ({bridge})"),
        Plan::AddressTakeover { address, port } => {
            format!("Adresse übernehmen {address} an {port}")
        }
        Plan::LeaseTakeover {
            name,
            address,
            router,
        } => format!("Lease übernehmen {name} {address} an {router}"),
        Plan::NewDevice { name, port } => format!("Neues Gerät {name} an {port}"),
        Plan::Flags { device } => format!("Flags {device}"),
        Plan::Wlan { ssid } => format!("WLAN {ssid}"),
        Plan::DeviceRoom { device } => format!("Raum von {device}"),
        Plan::PortVlan { port, vid } => match vid {
            Some(vid) => format!("VLAN {vid} an {port}"),
            None => format!("kein VLAN an {port}"),
        },
        Plan::Poe { pse, pd } => format!("PoE {pse} → {pd}"),
        Plan::PoeFeeding { port } => format!("PoE {port}"),
        Plan::NoPoe { port } => format!("kein PoE {port}"),
        Plan::RadioBridge { ap, station } => format!("Funkbrücke {ap} – {station}"),
        Plan::RadioDisconnect { devices } => {
            format!("Richtfunk abräumen {}", devices.join(" – "))
        }
    }
}

/// NetBox's changelog message of a write: what its plans do, and who wrote it.
pub fn message(plans: &[Plan], user: &str) -> String {
    let what = plans.iter().map(plan).collect::<Vec<_>>().join("; ");
    format!("{what} ({user})")
}

/// Who wrote, for [`message`]: a login acting for a resident.
pub fn for_resident(user: &str, resident: &str) -> String {
    format!("{user} für {resident}")
}

/// The description of a prefix created for an address taken over from a device.
pub fn address_taken_over(port: &Port) -> String {
    format!("Übernommen von {port}")
}

#[cfg(test)]
mod test {
    use super::{Plan, Port, message};

    fn port(device: &str, name: &str) -> Port {
        Port {
            device: device.into(),
            name: name.into(),
        }
    }

    #[test]
    fn a_write_names_its_plans_and_the_user() {
        let plans = [
            Plan::CableTakeover {
                connection: Box::new(Plan::Connection {
                    a: port("lab-np2", "ether3"),
                    b: port("lab-wap", "ether1"),
                }),
            },
            Plan::Loopback {
                device: "lab-wap".into(),
            },
        ];
        assert_eq!(
            message(&plans, "admin"),
            "Kabel übernehmen: Verbindung lab-np2 ether3 – lab-wap ether1; Loopback lab-wap (admin)"
        );
    }
}
