//! Scope: Tests of the schema's roots (`graphql`): who may call which root field; nothing else.
use super::create_schema;
use crate::{role::Role, topology::OwnerId};
use async_graphql::{Request, futures_util::StreamExt};
use serde_json::Value;

/// Root fields anyone may call, logged in or not.
const OPEN: &[&str] = &["authentication", "me"];
/// Root fields a resident may call (an admin too); every other one is an admin's.
const RESIDENT: &[&str] = &[
    "residents",
    "changePlan",
    "changes",
    "undoChange",
    "savedChange",
    "liveState",
    "networkDevices",
    "seenUplinks",
    "portDevices",
    "saveChange",
];

const INTROSPECTION: &str = r#"
fragment Ref on __Type { kind name ofType { kind name ofType { kind name ofType { kind name } } } }
{ __schema {
    queryType { name } mutationType { name } subscriptionType { name }
    types {
        kind name isOneOf
        fields { name args { name type { ...Ref } } type { ...Ref } }
        inputFields { name type { ...Ref } }
        enumValues { name }
    }
} }"#;

/// The schema's types by name, from introspection.
struct Types(Value);

impl Types {
    fn get(&self, name: &str) -> &Value {
        self.0["types"]
            .as_array()
            .expect("types")
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("type {name}"))
    }
    /// A literal of type `ty` that passes validation: the first enum value, an input object
    /// with its required fields (a `@oneOf` with its first), an empty list.
    fn literal(&self, ty: &Value) -> String {
        match ty["kind"].as_str().expect("kind") {
            "NON_NULL" => self.literal(&ty["ofType"]),
            "LIST" => "[]".into(),
            "ENUM" => self.get(ty["name"].as_str().expect("name"))["enumValues"][0]["name"]
                .as_str()
                .expect("value")
                .into(),
            "INPUT_OBJECT" => {
                let input = self.get(ty["name"].as_str().expect("name"));
                let fields = input["inputFields"].as_array().expect("fields");
                let chosen: Vec<_> = match input["isOneOf"].as_bool() == Some(true) {
                    true => fields.iter().take(1).collect(),
                    false => fields
                        .iter()
                        .filter(|f| f["type"]["kind"] == "NON_NULL")
                        .collect(),
                };
                let parts: Vec<_> = chosen
                    .iter()
                    .map(|f| {
                        format!(
                            "{}: {}",
                            f["name"].as_str().expect("name"),
                            self.literal(&f["type"])
                        )
                    })
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            _ => match ty["name"].as_str().expect("name") {
                "Int" | "Float" | "ScalarU64" | "ScalarDuration" => "0".into(),
                "ScalarMac" => "\"00:00:00:00:00:00\"".into(),
                "Boolean" => "false".into(),
                _ => "\"0\"".into(),
            },
        }
    }
    /// The operation calling root field `field` of `operation` with its required arguments.
    fn call(&self, operation: &str, field: &Value) -> String {
        let args: Vec<_> = field["args"]
            .as_array()
            .expect("args")
            .iter()
            .filter(|a| a["type"]["kind"] == "NON_NULL")
            .map(|a| {
                format!(
                    "{}: {}",
                    a["name"].as_str().expect("name"),
                    self.literal(&a["type"])
                )
            })
            .collect();
        let args = match args.is_empty() {
            true => String::new(),
            false => format!("({})", args.join(", ")),
        };
        let mut ty = &field["type"];
        while !ty["ofType"].is_null() {
            ty = &ty["ofType"];
        }
        let selection = match ty["kind"].as_str() {
            Some("OBJECT" | "INTERFACE" | "UNION") => " { __typename }",
            _ => "",
        };
        format!(
            "{operation} {{ {}{args}{selection} }}",
            field["name"].as_str().expect("name")
        )
    }
}

/// The code of the first error of calling `call` as `role` (none: without a role), or none.
async fn refusal(call: &str, subscription: bool, role: Option<Role>) -> Option<String> {
    let schema = create_schema(Default::default());
    let mut request = Request::new(call);
    if let Some(role) = role {
        request = request.data(role);
    }
    let response = match subscription {
        true => schema
            .execute_stream(request)
            .next()
            .await
            .expect("a first answer"),
        false => schema.execute(request).await,
    };
    let error = response.errors.first()?;
    Some(
        error
            .extensions
            .as_ref()
            .and_then(|extensions| extensions.get("code"))
            .map(|code| code.to_string().trim_matches('"').to_string())
            .unwrap_or_else(|| error.message.clone()),
    )
}

/// Every root field refuses a user without login and a resident, but those `OPEN` and, for a
/// resident, those `RESIDENT`: a root field without its guard fails here.
#[tokio::test]
async fn rights() {
    let schema = create_schema(Default::default());
    let introspection = schema.execute(INTROSPECTION).await;
    assert!(
        introspection.errors.is_empty(),
        "{:?}",
        introspection.errors
    );
    let data = introspection.data.into_json().expect("json");
    let types = Types(data["__schema"].clone());
    let mut seen = Vec::new();
    for (operation, root) in ["queryType", "mutationType", "subscriptionType"]
        .into_iter()
        .zip(["query", "mutation", "subscription"])
        .map(|(root, operation)| (operation, &data["__schema"][root]["name"]))
    {
        let root = types.get(root.as_str().expect("root"));
        for field in root["fields"].as_array().expect("fields") {
            let name = field["name"].as_str().expect("name");
            seen.push(name.to_string());
            if OPEN.contains(&name) {
                continue;
            }
            let call = types.call(operation, field);
            let subscription = operation == "subscription";
            let resident = Role::Resident {
                owners: Box::new([OwnerId(1)]),
            };
            for role in [None, Some(Role::Nobody), Some(resident)] {
                let is_resident = matches!(role, Some(Role::Resident { .. }));
                if is_resident && RESIDENT.contains(&name) {
                    continue;
                }
                assert_eq!(
                    refusal(&call, subscription, role.clone()).await.as_deref(),
                    Some("NOT_ALLOWED"),
                    "{call} as {role:?}"
                );
            }
        }
    }
    for listed in OPEN.iter().chain(RESIDENT) {
        assert!(
            seen.iter().any(|name| name == listed),
            "{listed} is no root field"
        );
    }
}
