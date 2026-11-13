//! Scope: A device's answer built by hand for tests, read as its model; not the tests themselves.

use mikrotik_model::resource::{AppendFieldResult, DeserializeRosBuilder, DeserializeRosResource};

/// The row `fields` as the model `R` reads it from a device: a field the model doesn't know is
/// passed over, as on a device (a row copied from one keeps its `.id`), a value it can't read
/// fails, as does a field it needs that `fields` lacks.
pub(crate) fn parsed<R: DeserializeRosResource>(fields: &[(&str, &str)]) -> R
where
    <R::Builder as DeserializeRosBuilder<R>>::Context: Default,
{
    let mut builder = R::Builder::init(&Default::default());
    for (name, value) in fields {
        let appended = builder.append_field(name.as_bytes(), Some(value.as_bytes()));
        assert!(
            matches!(
                appended,
                AppendFieldResult::Appended | AppendFieldResult::UnknownField
            ),
            "{name}={value}: {appended:?}"
        );
    }
    builder
        .build()
        .unwrap_or_else(|missing| panic!("missing {}", String::from_utf8_lossy(missing)))
}

/// [`parsed`], with `defaults` for the fields `fields` doesn't give.
pub(crate) fn parsed_with<R: DeserializeRosResource>(
    fields: &[(&str, &str)],
    defaults: &[(&str, &str)],
) -> R
where
    <R::Builder as DeserializeRosBuilder<R>>::Context: Default,
{
    let given = |key: &str| fields.iter().any(|(k, _)| *k == key);
    let all = fields
        .iter()
        .chain(defaults.iter().filter(|(key, _)| !given(key)))
        .copied()
        .collect::<Vec<_>>();
    parsed(&all)
}
