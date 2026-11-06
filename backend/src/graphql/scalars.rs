use async_graphql::{InputValueError, InputValueResult, Scalar, ScalarType, Value};
use mikrotik_model::MacAddress;
use std::{str::FromStr, time::Duration};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ScalarDuration(Duration);

impl From<Duration> for ScalarDuration {
    fn from(duration: Duration) -> Self {
        ScalarDuration(duration)
    }
}

impl From<ScalarDuration> for Duration {
    fn from(duration: ScalarDuration) -> Self {
        duration.0
    }
}

#[Scalar]
impl ScalarType for ScalarDuration {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::Number(n) => match n.as_u64().map(|v| Self(Duration::from_nanos(v))) {
                None => Err(InputValueError::custom(format!(
                    "Invalid value for ScalarDuration: {}",
                    n
                ))),
                Some(v) => Ok(v),
            },
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::Number((self.0.as_nanos() as u64).into())
    }
}

/// A number that may go past the 32 bits of GraphQL's `Int` (bytes, the hosts of an IPv6 pool),
/// as a JSON number.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct ScalarU64(pub u64);

impl From<u64> for ScalarU64 {
    fn from(value: u64) -> Self {
        ScalarU64(value)
    }
}

#[Scalar]
impl ScalarType for ScalarU64 {
    fn parse(value: Value) -> InputValueResult<Self> {
        match &value {
            Value::Number(n) => n
                .as_u64()
                .map(ScalarU64)
                .ok_or_else(|| InputValueError::custom(format!("not a u64: {n}"))),
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::Number(self.0.into())
    }
}

/// A MAC address, as text `D4:01:C3:29:67:64`; also read with `-` and around whitespace.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ScalarMac(pub MacAddress);

impl From<MacAddress> for ScalarMac {
    fn from(mac: MacAddress) -> Self {
        ScalarMac(mac)
    }
}

#[Scalar]
impl ScalarType for ScalarMac {
    fn parse(value: Value) -> InputValueResult<Self> {
        match &value {
            Value::String(text) => MacAddress::from_str(text.trim())
                .map(ScalarMac)
                .map_err(|_| InputValueError::custom(format!("not a MAC address: {text}"))),
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::ScalarMac;
    use async_graphql::{ScalarType, Value};

    fn parse(text: &str) -> Option<String> {
        ScalarMac::parse(Value::String(text.into()))
            .ok()
            .map(|mac| mac.0.to_string())
    }

    #[test]
    fn parses_macs() {
        assert_eq!(
            parse(" d4:01:c3:29:67:64 ").as_deref(),
            Some("D4:01:C3:29:67:64")
        );
        assert_eq!(
            parse("D4-01-C3-29-67-64").as_deref(),
            Some("D4:01:C3:29:67:64")
        );
        assert_eq!(
            parse("00:0a:00:01:00:0f").as_deref(),
            Some("00:0A:00:01:00:0F")
        );
        assert_eq!(parse("D4:01:C3:29:67"), None);
        assert_eq!(parse("D4:01:C3:29:67:6G"), None);
    }
}
