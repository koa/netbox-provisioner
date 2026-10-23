//! Errors as the API reports them: in the extensions of a GraphQL error (`ApiError`), as a
//! job's `error`, in job events and as `bootstrapProblem`. `code` names the case, `params` hold
//! the values a text about it needs; the frontend words it by `code` and shows `message`, the
//! English `Display` (also what the log has), only for a code it doesn't know yet.
use crate::{Error, device::ros::SetupError};
use async_graphql::{ErrorExtensions, Name, SimpleObject, Value};

#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct ErrorInfo {
    pub code: &'static str,
    pub params: Vec<ErrorParam>,
    pub message: Box<str>,
}

#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct ErrorParam {
    pub name: &'static str,
    pub value: Box<str>,
}

/// Why a device has no way to its initial setup (`Error::NoBootstrapPath`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapObstacle {
    /// no port with an IPv4 address in NetBox, on the port or its bridge
    NoIpv4Port,
    /// no cable from such a port to a device the provisioner reaches, in the same network
    NoReachableNeighbor,
}

impl BootstrapObstacle {
    /// its name in the `obstacle` param
    fn code(self) -> &'static str {
        match self {
            BootstrapObstacle::NoIpv4Port => "NO_IPV4_PORT",
            BootstrapObstacle::NoReachableNeighbor => "NO_REACHABLE_NEIGHBOR",
        }
    }
}

impl std::fmt::Display for BootstrapObstacle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BootstrapObstacle::NoIpv4Port => "no port with an IPv4 address in NetBox",
            BootstrapObstacle::NoReachableNeighbor => {
                "no cable from a port with IPv4 address to a reachable device"
            }
        })
    }
}

impl From<&Error> for ErrorInfo {
    fn from(error: &Error) -> Self {
        let cause = |cause: &dyn std::fmt::Display| vec![("cause", cause.to_string())];
        let (code, params) = match error {
            Error::SetupError(error) => return error.into(),
            Error::MikrotikApi(error) => ("DEVICE_API", cause(error)),
            Error::MikrotikModel(error) => ("DEVICE_MODEL", cause(error)),
            Error::MissingCredentials => ("MISSING_CREDENTIALS", vec![]),
            Error::AddressParse(error) => ("INVALID_ADDRESS", cause(error)),
            Error::ResourceMutation(error) => ("MUTATIONS", cause(error)),
            Error::MissingDependenciesError(detail) => ("MISSING_DEPENDENCIES", cause(detail)),
            Error::ErrorGeneratingString(error) => ("SCRIPT", cause(error)),
            Error::Proxy(address, error) => (
                "PROXY",
                vec![("address", address.to_string()), ("cause", error.to_string())],
            ),
            Error::ApplyNotAllowed => ("APPLY_NOT_ALLOWED", vec![]),
            Error::UnknownDevice(id) => ("UNKNOWN_DEVICE", vec![("id", id.to_string())]),
            Error::UnknownInterface(id) => ("UNKNOWN_INTERFACE", vec![("id", id.to_string())]),
            Error::NoTopology => ("NO_TOPOLOGY", vec![]),
            Error::Timeout(address) => ("TIMEOUT", vec![("address", address.to_string())]),
            Error::NoAddress(device) => ("NO_ADDRESS", vec![("device", device.to_string())]),
            Error::Terminal(error) => ("CONSOLE", cause(error)),
            Error::NoBootstrapPath(device, obstacle) => (
                "NO_BOOTSTRAP_PATH",
                vec![
                    ("device", device.to_string()),
                    ("obstacle", obstacle.code().into()),
                ],
            ),
            Error::ConsoleLoginRefused(macs) => {
                ("CONSOLE_LOGIN_REFUSED", vec![("macs", macs.to_string())])
            }
            Error::ConsoleNoAnswer(macs) => ("CONSOLE_NO_ANSWER", vec![("macs", macs.to_string())]),
            Error::SetupCommand(command, error) => (
                "SETUP_COMMAND",
                vec![("command", command.to_string()), ("cause", error.to_string())],
            ),
            Error::NeighborNoAnswer(neighbor, address) => (
                "NEIGHBOR_NO_ANSWER",
                vec![("neighbor", neighbor.to_string()), ("address", address.to_string())],
            ),
            Error::NeighborNoForwarding(neighbor, forwarding) => (
                "NEIGHBOR_NO_FORWARDING",
                vec![
                    ("neighbor", neighbor.to_string()),
                    ("forwarding", forwarding.to_string()),
                ],
            ),
            Error::ConsoleSessionEnded(command) => {
                ("CONSOLE_SESSION_ENDED", vec![("command", command.to_string())])
            }
            Error::Refused(error) => ("REFUSED", cause(error)),
            Error::CannotRepartition(count) => {
                ("CANNOT_REPARTITION", vec![("count", count.to_string())])
            }
            Error::DeviceModeNotConfirmed => ("DEVICE_MODE_NOT_CONFIRMED", vec![]),
            Error::DeviceModeUnchanged(features) => {
                ("DEVICE_MODE_UNCHANGED", vec![("features", features.to_string())])
            }
            Error::Download(url, error) => (
                "DOWNLOAD",
                vec![("url", url.to_string()), ("cause", error.to_string())],
            ),
            Error::UnknownRelease(url, answer) => (
                "UNKNOWN_RELEASE",
                vec![("url", url.to_string()), ("answer", answer.to_string())],
            ),
            Error::MissingDeviceState(field) => {
                ("MISSING_DEVICE_STATE", vec![("field", field.to_string())])
            }
            Error::UploadMissing(file) => ("UPLOAD_MISSING", vec![("file", file.to_string())]),
            Error::UploadIncomplete {
                file,
                size,
                expected,
            } => (
                "UPLOAD_INCOMPLETE",
                vec![
                    ("file", file.to_string()),
                    ("size", size.to_string()),
                    ("expected", expected.to_string()),
                ],
            ),
            Error::UnexpectedAfterReboot { version, firmware } => (
                "UNEXPECTED_AFTER_REBOOT",
                [("version", version.to_string())]
                    .into_iter()
                    .chain(firmware.iter().map(|firmware| ("firmware", firmware.to_string())))
                    .collect(),
            ),
            Error::NotBackAfterReboot => ("NOT_BACK_AFTER_REBOOT", vec![]),
            Error::UpdateRolledBack { partition, version } => (
                "UPDATE_ROLLED_BACK",
                vec![("partition", partition.to_string()), ("version", version.to_string())],
            ),
            Error::OspfNeighborsMissing(neighbors) => {
                ("OSPF_NEIGHBORS_MISSING", vec![("neighbors", neighbors.to_string())])
            }
            Error::Busy(device) => ("BUSY", vec![("device", device.to_string())]),
            Error::BatchStopped(device) => ("BATCH_STOPPED", vec![("device", device.to_string())]),
            Error::ScriptNotStarted(waited) => {
                ("SCRIPT_NOT_STARTED", vec![("seconds", waited.as_secs().to_string())])
            }
            Error::BatchIncomplete(devices) => {
                ("BATCH_INCOMPLETE", vec![("devices", devices.to_string())])
            }
            Error::UnknownJob(id) => ("UNKNOWN_JOB", vec![("id", id.to_string())]),
            Error::Cancelled => ("CANCELLED", vec![]),
            Error::Panic(message) => ("PANIC", vec![("message", message.to_string())]),
            Error::Ping(error) => ("PING", cause(error)),
            Error::Netbox(crate::netbox::NetboxError::WriteNotAllowed) => {
                ("NETBOX_WRITE_NOT_ALLOWED", vec![])
            }
            Error::Netbox(error) => ("NETBOX", cause(error)),
            Error::PlanOutdated => ("PLAN_OUTDATED", vec![]),
            Error::NetboxChanged(what) => ("NETBOX_CHANGED", vec![("what", what.to_string())]),
            Error::NoPreview => ("NO_PREVIEW", vec![]),
            Error::Preview(error) => ("PREVIEW", cause(error)),
            Error::NoFreeAddress(pool, version) => (
                "NO_FREE_ADDRESS",
                vec![("pool", pool.to_string()), ("version", version.to_string())],
            ),
            Error::NoRouteCheck(reason) => ("NO_ROUTE_CHECK", vec![("reason", reason.to_string())]),
            Error::NothingToUndo(job) => ("NOTHING_TO_UNDO", vec![("job", job.to_string())]),
            Error::SameDevice(device) => ("SAME_DEVICE", vec![("device", device.to_string())]),
            Error::NoRouterOs(device) => ("NO_ROUTEROS", vec![("device", device.to_string())]),
            Error::PortCabled { port, far } => {
                let mut params = vec![("port", port.to_string())];
                params.extend(far.as_ref().map(|far| ("far", far.to_string())));
                ("PORT_CABLED", params)
            }
            Error::PortInBridge { port, bridge } => (
                "PORT_IN_BRIDGE",
                vec![("port", port.to_string()), ("bridge", bridge.to_string())],
            ),
            Error::PortAddressed { port, address } => (
                "PORT_ADDRESSED",
                vec![("port", port.to_string()), ("address", address.to_string())],
            ),
            Error::TagMissing(tag) => ("TAG_MISSING", vec![("tag", tag.to_string())]),
            Error::PrimaryAddress { address, device } => (
                "PRIMARY_ADDRESS",
                vec![("address", address.to_string()), ("device", device.to_string())],
            ),
            Error::NothingToDisconnect(port) => {
                ("NOTHING_TO_DISCONNECT", vec![("port", port.to_string())])
            }
            Error::NoCustomerNet(interface) => {
                ("NO_CUSTOMER_NET", vec![("interface", interface.to_string())])
            }
            Error::NoSite(device) => ("NO_SITE", vec![("device", device.to_string())]),
            Error::UnknownPrefix(id) => ("UNKNOWN_PREFIX", vec![("id", id.to_string())]),
            Error::UnknownVlan(id) => ("UNKNOWN_VLAN", vec![("id", id.to_string())]),
            Error::NetworkLength { length, parent } => (
                "NETWORK_LENGTH",
                vec![("length", length.to_string()), ("parent", parent.to_string())],
            ),
            Error::NoParent => ("NO_PARENT", Vec::new()),
            Error::NoFreeNetwork { parent, length } => (
                "NO_FREE_NETWORK",
                vec![("parent", parent.to_string()), ("length", length.to_string())],
            ),
            Error::NotEthernet(port) => ("NOT_ETHERNET", vec![("port", port.to_string())]),
            Error::NoSingleRadio(device) => ("NO_SINGLE_RADIO", vec![("device", device.to_string())]),
            Error::NoRouterPort(device) => ("NO_ROUTER_PORT", vec![("device", device.to_string())]),
            Error::WirelessLinkElsewhere { port, far } => {
                let mut params = vec![("port", port.to_string())];
                params.extend(far.as_ref().map(|far| ("far", far.to_string())));
                ("WIRELESS_LINK_ELSEWHERE", params)
            }
            Error::PskUnreadable(port) => ("PSK_UNREADABLE", vec![("port", port.to_string())]),
            Error::NetworkTooSmall { net, ends } => (
                "NETWORK_TOO_SMALL",
                vec![("net", net.to_string()), ("ends", ends.to_string())],
            ),
            Error::NameTaken { device, name } => (
                "NAME_TAKEN",
                vec![("device", device.to_string()), ("name", name.to_string())],
            ),
            Error::WatchdogNotSuspended { device, cause } => (
                "WATCHDOG_NOT_SUSPENDED",
                vec![("device", device.to_string()), ("cause", cause.to_string())],
            ),
            Error::WrongDevice {
                device,
                address,
                other,
            } => (
                "WRONG_DEVICE",
                vec![
                    ("device", device.to_string()),
                    ("address", address.to_string()),
                    ("other", other.to_string()),
                ],
            ),
        };
        info(code, params, error)
    }
}

impl From<&SetupError> for ErrorInfo {
    fn from(error: &SetupError) -> Self {
        let one = |name, value: &dyn std::fmt::Display| vec![(name, value.to_string())];
        let (code, params) = match error {
            SetupError::Access(error) => ("DEVICE_MODEL", one("cause", error)),
            SetupError::RouterboardNotDefined => ("NO_ROUTERBOARD", vec![]),
            SetupError::NoPortsFound(model) => ("NO_PORTS", one("model", model)),
            SetupError::PortNotFound(port) => ("PORT_NOT_FOUND", one("port", port)),
            SetupError::MissingPrefixOnIpAddress { ip, interface } => (
                "NO_PREFIX",
                vec![("net", ip.to_string()), ("interface", interface.to_string())],
            ),
            SetupError::MissingAddressOnPrefix { prefix } => {
                ("NO_ADDRESS_ON_PREFIX", one("prefix", prefix))
            }
            SetupError::MissingApMgmtIf => ("NO_AP_MGMT_INTERFACE", vec![]),
            SetupError::AmbiguousVlanIds(_) => ("AMBIGUOUS_VLAN_IDS", vec![]),
            SetupError::VlanOnCableAndTunnel {
                vlan,
                interface,
                far_end,
            } => (
                "VLAN_LOOP",
                vec![
                    ("vlan", vlan.to_string()),
                    ("interface", interface.to_string()),
                    ("farEnd", far_end.clone()),
                ],
            ),
            SetupError::MultipleNetworksOnVlan { vlan, networks } => (
                "NETWORKS_ON_VLAN",
                vec![("vlan", vlan.to_string()), ("networks", networks.join(", "))],
            ),
            SetupError::MultipleDhcpServersOnVlan { vlan, servers } => (
                "DHCP_SERVERS_ON_VLAN",
                vec![("vlan", vlan.to_string()), ("servers", servers.join(", "))],
            ),
            SetupError::MissingVtepAddress(vxlan) => ("NO_VTEP_ADDRESS", one("vxlan", vxlan)),
            SetupError::PortInMultipleBridges(port) => ("PORT_IN_BRIDGES", one("port", port)),
            SetupError::UnsupportedChannelWidth(width) => ("CHANNEL_WIDTH", one("width", width)),
            SetupError::NoRadioProfile(radio) => ("NO_RADIO_PROFILE", one("radio", radio)),
            SetupError::MissingWirelessLink(radio) => ("NO_WIRELESS_LINK", one("radio", radio)),
            SetupError::MissingPsk(link) => ("NO_PSK", one("link", link)),
            SetupError::MissingCountry(device) => ("NO_COUNTRY", one("device", device)),
            SetupError::UnknownCountry(region) => ("UNKNOWN_COUNTRY", one("region", region)),
            SetupError::InvalidFrequencyMode(mode) => ("FREQUENCY_MODE", one("mode", mode)),
            SetupError::MissingPackage(package) => ("MISSING_PACKAGE", one("package", package)),
            SetupError::MissingController(device) => ("NO_CONTROLLER", one("device", device)),
            SetupError::WlanVlansNotTagged(device) => ("WLAN_VLANS_UNTAGGED", one("device", device)),
            SetupError::MtuMismatch {
                port,
                mtu,
                far_end,
                far_mtu,
            } => (
                "MTU_MISMATCH",
                vec![
                    ("port", port.to_string()),
                    ("mtu", mtu.to_string()),
                    ("farEnd", far_end.clone()),
                    ("farMtu", far_mtu.to_string()),
                ],
            ),
            SetupError::MtuNotSupported(port) => ("MTU_NOT_SUPPORTED", one("port", port)),
            SetupError::SpeedNotSupported(port) => ("SPEED_NOT_SUPPORTED", one("port", port)),
            SetupError::SpeedNeedsCableType { port, speed } => (
                "SPEED_NEEDS_CABLE_TYPE",
                vec![("port", port.to_string()), ("speed", speed.clone())],
            ),
            SetupError::UnknownSpeed { port, speed } => (
                "UNKNOWN_SPEED",
                vec![("port", port.to_string()), ("speed", speed.clone())],
            ),
            SetupError::SpeedNotOnPort { port, speed } => (
                "SPEED_NOT_ON_PORT",
                vec![("port", port.to_string()), ("speed", speed.clone())],
            ),
            SetupError::SpeedMismatch {
                port,
                speed,
                far_end,
                far_speed,
            } => (
                "SPEED_MISMATCH",
                vec![
                    ("port", port.to_string()),
                    ("speed", speed.clone()),
                    ("farEnd", far_end.to_string()),
                    ("farSpeed", far_speed.as_deref().unwrap_or_default().to_string()),
                ],
            ),
        };
        info(code, params, error)
    }
}

fn info(code: &'static str, params: Vec<(&'static str, String)>, error: &dyn std::fmt::Display) -> ErrorInfo {
    ErrorInfo {
        code,
        params: params
            .into_iter()
            .map(|(name, value)| ErrorParam {
                name,
                value: value.into(),
            })
            .collect(),
        message: error.to_string().into(),
    }
}

/// What a resolver returns instead of [`Error`], so that the GraphQL error carries
/// [`ErrorInfo`]'s `code` and `params` in its extensions (async-graphql would turn an
/// `Error` into its message only).
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        ApiError(error)
    }
}
impl From<SetupError> for ApiError {
    fn from(error: SetupError) -> Self {
        ApiError(error.into())
    }
}

impl From<ApiError> for async_graphql::Error {
    fn from(ApiError(error): ApiError) -> Self {
        let info = ErrorInfo::from(&error);
        async_graphql::Error::new(info.message.to_string()).extend_with(|_, extensions| {
            extensions.set("code", info.code);
            extensions.set(
                "params",
                Value::Object(
                    info.params
                        .iter()
                        .map(|param| (Name::new(param.name), Value::from(param.value.as_ref())))
                        .collect(),
                ),
            );
        })
    }
}

#[cfg(test)]
mod test {
    use super::{ApiError, ErrorInfo};
    use crate::{Error, device::ros::SetupError};
    use async_graphql::Value;

    #[test]
    fn setup_errors_keep_their_own_code() {
        let info = ErrorInfo::from(&Error::from(SetupError::MissingPackage("wifi-qcom")));
        assert_eq!(info.code, "MISSING_PACKAGE");
        assert_eq!(info.params[0].name, "package");
        assert_eq!(info.params[0].value.as_ref(), "wifi-qcom");
    }

    /// a GraphQL error carries code and params, not only the message
    #[test]
    fn graphql_error_has_code_and_params() {
        let error = async_graphql::Error::from(ApiError(Error::Busy("lab-ap1".into())));
        assert_eq!(error.message, "lab-ap1 runs a job already");
        let extensions = error.extensions.expect("extensions");
        assert_eq!(extensions.get("code"), Some(&Value::from("BUSY")));
        let Some(Value::Object(params)) = extensions.get("params") else {
            panic!("no params");
        };
        assert_eq!(params.get("device"), Some(&Value::from("lab-ap1")));
    }
}
