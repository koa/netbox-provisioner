use async_graphql::{InputValueError, InputValueResult, Scalar, ScalarType, Value};
use std::time::Duration;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ScalarDuration(Duration);

impl From<Duration> for ScalarDuration {
    fn from(duration: Duration) -> Self {
        ScalarDuration(duration)
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
