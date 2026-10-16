//! What applying the generated config would do, or leave, that the operator should know before
//! (`AccessibleDevice.generateCfg`). Data only: the frontend words it.
use async_graphql::{SimpleObject, Union};

#[derive(Clone, Debug, PartialEq, Union)]
pub enum ApplyWarning {
    PoeCut(PoeCut),
    RadioBound(RadioBound),
    LegacyRadioManaged(LegacyRadioManaged),
}

/// The PoE out of `port` powers something, and the target switches it off (NetBox has no
/// `poe_mode=pse` there).
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct PoeCut {
    /// the name the target gives the port
    pub port: Box<str>,
    pub default_name: Box<str>,
}

/// CAPsMAN has bound `radio`, which RouterOS then refuses to change, and it uses the
/// configuration `configuration` instead of `expected` (NetBox's).
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct RadioBound {
    pub radio: Box<str>,
    pub configuration: Option<Box<str>>,
    pub expected: Box<str>,
}

/// CAPsMAN manages the legacy radio `radio` (its name on the device), which RouterOS then
/// refuses to change: it keeps its name and settings, NetBox's name for it is `expected`.
#[derive(Clone, Debug, PartialEq, SimpleObject)]
pub struct LegacyRadioManaged {
    pub radio: Box<str>,
    pub expected: Box<str>,
}
