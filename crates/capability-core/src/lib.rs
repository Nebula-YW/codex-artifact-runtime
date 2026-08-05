use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const CATALOG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityCatalog {
    pub schema_version: u32,
    pub namespaces: Vec<CapabilityNamespace>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityNamespace {
    pub name: String,
    pub description: String,
    pub operations: Vec<CapabilityOperation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityOperation {
    pub name: String,
    pub description: String,
    pub kind: OperationKind,
    pub input_schema: Value,
    pub output_schema: Value,
    #[serde(default)]
    pub risk: OperationRisk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Request,
    Stream,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationRisk {
    #[default]
    Read,
    Write,
    ExternalSideEffect,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CodexDynamicToolSpec {
    Namespace {
        name: String,
        description: String,
        tools: Vec<CodexDynamicNamespaceTool>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CodexDynamicNamespaceTool {
    #[serde(rename = "type")]
    pub kind: CodexFunctionKind,
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(rename = "deferLoading")]
    pub defer_loading: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CodexFunctionKind {
    Function,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicToolCallParams {
    pub thread_id: String,
    pub turn_id: String,
    pub call_id: String,
    pub namespace: Option<String>,
    pub tool: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicToolCallResponse {
    pub content_items: Vec<DynamicToolCallOutputContentItem>,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DynamicToolCallOutputContentItem {
    InputText { text: String },
    InputImage { image_url: String },
    InputAudio { audio_url: String },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CatalogError {
    #[error("unsupported capability catalog schema version {actual}; expected {expected}")]
    UnsupportedVersion { actual: u32, expected: u32 },
    #[error("invalid capability {kind} name {name:?}; use an ASCII identifier")]
    InvalidName { kind: &'static str, name: String },
    #[error("duplicate capability namespace {0:?}")]
    DuplicateNamespace(String),
    #[error("duplicate operation {namespace}.{operation}")]
    DuplicateOperation {
        namespace: String,
        operation: String,
    },
    #[error("{namespace}.{operation} {position} schema must be a JSON object")]
    InvalidSchema {
        namespace: String,
        operation: String,
        position: &'static str,
    },
    #[error("unknown capability operation {namespace}.{operation}")]
    UnknownOperation {
        namespace: String,
        operation: String,
    },
    #[error("{namespace}.{operation} {position} failed schema validation at {path}: {message}")]
    SchemaViolation {
        namespace: String,
        operation: String,
        position: &'static str,
        path: String,
        message: String,
    },
}

impl CapabilityCatalog {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.schema_version != CATALOG_SCHEMA_VERSION {
            return Err(CatalogError::UnsupportedVersion {
                actual: self.schema_version,
                expected: CATALOG_SCHEMA_VERSION,
            });
        }
        let mut namespaces = BTreeSet::new();
        for namespace in &self.namespaces {
            validate_name("namespace", &namespace.name)?;
            if !namespaces.insert(namespace.name.as_str()) {
                return Err(CatalogError::DuplicateNamespace(namespace.name.clone()));
            }
            let mut operations = BTreeSet::new();
            for operation in &namespace.operations {
                validate_name("operation", &operation.name)?;
                if !operations.insert(operation.name.as_str()) {
                    return Err(CatalogError::DuplicateOperation {
                        namespace: namespace.name.clone(),
                        operation: operation.name.clone(),
                    });
                }
                for (position, schema) in [
                    ("input", &operation.input_schema),
                    ("output", &operation.output_schema),
                ] {
                    if !schema.is_object() {
                        return Err(CatalogError::InvalidSchema {
                            namespace: namespace.name.clone(),
                            operation: operation.name.clone(),
                            position,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    pub fn operation(
        &self,
        namespace: &str,
        operation: &str,
    ) -> Result<&CapabilityOperation, CatalogError> {
        self.namespaces
            .iter()
            .find(|candidate| candidate.name == namespace)
            .and_then(|candidate| {
                candidate
                    .operations
                    .iter()
                    .find(|candidate| candidate.name == operation)
            })
            .ok_or_else(|| CatalogError::UnknownOperation {
                namespace: namespace.to_string(),
                operation: operation.to_string(),
            })
    }

    pub fn codex_dynamic_tools(
        &self,
        defer_loading: bool,
    ) -> Result<Vec<CodexDynamicToolSpec>, CatalogError> {
        self.validate()?;
        Ok(self
            .namespaces
            .iter()
            .filter_map(|namespace| {
                let tools = namespace
                    .operations
                    .iter()
                    .filter(|operation| operation.kind == OperationKind::Request)
                    .map(|operation| CodexDynamicNamespaceTool {
                        kind: CodexFunctionKind::Function,
                        name: operation.name.clone(),
                        description: operation.description.clone(),
                        input_schema: operation.input_schema.clone(),
                        defer_loading,
                    })
                    .collect::<Vec<_>>();
                (!tools.is_empty()).then(|| CodexDynamicToolSpec::Namespace {
                    name: namespace.name.clone(),
                    description: namespace.description.clone(),
                    tools,
                })
            })
            .collect())
    }

    pub fn validate_input(
        &self,
        namespace: &str,
        operation: &str,
        value: &Value,
    ) -> Result<(), CatalogError> {
        let definition = self.operation(namespace, operation)?;
        validate_json(
            &definition.input_schema,
            value,
            "$",
            namespace,
            operation,
            "input",
        )
    }

    pub fn validate_output(
        &self,
        namespace: &str,
        operation: &str,
        value: &Value,
    ) -> Result<(), CatalogError> {
        let definition = self.operation(namespace, operation)?;
        validate_json(
            &definition.output_schema,
            value,
            "$",
            namespace,
            operation,
            "output",
        )
    }

    pub fn typescript_declarations(&self) -> Result<String, CatalogError> {
        self.validate()?;
        let mut output = String::from(
            "// Generated from the Capability Catalog. Do not edit.\n\nexport interface CapabilityCallOptions {\n  signal?: AbortSignal;\n}\n\nexport interface CapabilityStream<T> extends AsyncIterable<T> {\n  close(): Promise<void>;\n}\n\n",
        );
        for namespace in &self.namespaces {
            for operation in &namespace.operations {
                let prefix = format!(
                    "{}{}",
                    pascal_case(&namespace.name),
                    pascal_case(&operation.name)
                );
                output.push_str(&format!(
                    "export type {prefix}Input = {};\n",
                    schema_to_typescript(&operation.input_schema)
                ));
                output.push_str(&format!(
                    "export type {prefix}Output = {};\n\n",
                    schema_to_typescript(&operation.output_schema)
                ));
            }
        }
        output.push_str("export interface CapabilityClient {\n");
        for namespace in &self.namespaces {
            output.push_str(&format!("  {}: {{\n", namespace.name));
            for operation in &namespace.operations {
                let prefix = format!(
                    "{}{}",
                    pascal_case(&namespace.name),
                    pascal_case(&operation.name)
                );
                let result = match operation.kind {
                    OperationKind::Request => format!("Promise<{prefix}Output>"),
                    OperationKind::Stream => format!("CapabilityStream<{prefix}Output>"),
                };
                output.push_str(&format!(
                    "    {}(input: {prefix}Input, options?: CapabilityCallOptions): {result};\n",
                    operation.name
                ));
            }
            output.push_str("  };\n");
        }
        output.push_str("}\n\nexport declare const tools: CapabilityClient;\n");
        Ok(output)
    }
}

fn validate_name(kind: &'static str, name: &str) -> Result<(), CatalogError> {
    let mut chars = name.chars();
    let valid = chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if valid {
        Ok(())
    } else {
        Err(CatalogError::InvalidName {
            kind,
            name: name.to_string(),
        })
    }
}

fn validate_json(
    schema: &Value,
    value: &Value,
    path: &str,
    namespace: &str,
    operation: &str,
    position: &'static str,
) -> Result<(), CatalogError> {
    let fail = |message: String| CatalogError::SchemaViolation {
        namespace: namespace.to_string(),
        operation: operation.to_string(),
        position,
        path: path.to_string(),
        message,
    };
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(value)
    {
        return Err(fail("value is not in enum".to_string()));
    }
    if let Some(expected) = schema.get("type").and_then(Value::as_str) {
        let matches = match expected {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        };
        if !matches {
            return Err(fail(format!("expected {expected}")));
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for name in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(name) {
                    return Err(fail(format!("missing required property {name:?}")));
                }
            }
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (name, property_schema) in properties {
                if let Some(property) = object.get(name) {
                    validate_json(
                        property_schema,
                        property,
                        &format!("{path}.{name}"),
                        namespace,
                        operation,
                        position,
                    )?;
                }
            }
        }
    }
    if let (Some(items), Some(values)) = (schema.get("items"), value.as_array()) {
        for (index, item) in values.iter().enumerate() {
            validate_json(
                items,
                item,
                &format!("{path}[{index}]"),
                namespace,
                operation,
                position,
            )?;
        }
    }
    Ok(())
}

fn schema_to_typescript(schema: &Value) -> String {
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return values
            .iter()
            .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "unknown".to_string()))
            .collect::<Vec<_>>()
            .join(" | ");
    }
    if let Some(options) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
    {
        return options
            .iter()
            .map(schema_to_typescript)
            .collect::<Vec<_>>()
            .join(" | ");
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => "string".to_string(),
        Some("integer" | "number") => "number".to_string(),
        Some("boolean") => "boolean".to_string(),
        Some("null") => "null".to_string(),
        Some("array") => format!(
            "Array<{}>",
            schema
                .get("items")
                .map(schema_to_typescript)
                .unwrap_or_else(|| "unknown".to_string())
        ),
        Some("object") => {
            let required = schema
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>();
            let fields = schema
                .get("properties")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .map(|(name, field)| {
                    let optional = if required.contains(name.as_str()) {
                        ""
                    } else {
                        "?"
                    };
                    format!(
                        "{}{}: {}",
                        typescript_property(name),
                        optional,
                        schema_to_typescript(field)
                    )
                })
                .collect::<Vec<_>>();
            if fields.is_empty() {
                "Record<string, unknown>".to_string()
            } else {
                format!("{{ {} }}", fields.join("; "))
            }
        }
        _ => "unknown".to_string(),
    }
}

fn typescript_property(name: &str) -> String {
    if validate_name("property", name).is_ok() {
        name.to_string()
    } else {
        serde_json::to_string(name).unwrap_or_else(|_| "\"field\"".to_string())
    }
}

fn pascal_case(value: &str) -> String {
    let mut output = String::new();
    let mut uppercase = true;
    for character in value.chars() {
        if character == '_' {
            uppercase = true;
        } else if uppercase {
            output.push(character.to_ascii_uppercase());
            uppercase = false;
        } else {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn catalog() -> CapabilityCatalog {
        CapabilityCatalog {
            schema_version: 1,
            namespaces: vec![CapabilityNamespace {
                name: "autoComm".to_string(),
                description: "Vehicle communication".to_string(),
                operations: vec![
                    CapabilityOperation {
                        name: "call".to_string(),
                        description: "Call a service".to_string(),
                        kind: OperationKind::Request,
                        input_schema: json!({
                            "type": "object",
                            "properties": {"selector": {"type": "string"}},
                            "required": ["selector"]
                        }),
                        output_schema: json!({
                            "type": "object",
                            "properties": {"value": {"type": "number"}},
                            "required": ["value"]
                        }),
                        risk: OperationRisk::Read,
                    },
                    CapabilityOperation {
                        name: "watch".to_string(),
                        description: "Watch values".to_string(),
                        kind: OperationKind::Stream,
                        input_schema: json!({"type": "object"}),
                        output_schema: json!({"type": "object"}),
                        risk: OperationRisk::Read,
                    },
                ],
            }],
        }
    }

    #[test]
    fn projects_only_request_operations_to_codex() {
        let projection = catalog().codex_dynamic_tools(true).unwrap();
        let value = serde_json::to_value(projection).unwrap();
        assert_eq!(value[0]["type"], "namespace");
        assert_eq!(value[0]["tools"][0]["name"], "call");
        assert_eq!(value[0]["tools"][0]["deferLoading"], true);
        assert_eq!(value[0]["tools"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn validates_required_input_fields() {
        let error = catalog()
            .validate_input("autoComm", "call", &json!({}))
            .unwrap_err();
        assert!(error.to_string().contains("selector"));
        catalog()
            .validate_input(
                "autoComm",
                "call",
                &json!({"selector": "VehicleStatus.GetSpeed"}),
            )
            .unwrap();
    }

    #[test]
    fn generates_shared_typescript_surface() {
        let declarations = catalog().typescript_declarations().unwrap();
        assert!(declarations.contains("autoComm: {"));
        assert!(declarations.contains("call(input: AutoCommCallInput"));
        assert!(declarations.contains("Promise<AutoCommCallOutput>"));
        assert!(declarations.contains("watch(input: AutoCommWatchInput"));
        assert!(declarations.contains("CapabilityStream<AutoCommWatchOutput>"));
    }

    #[test]
    fn rejects_names_that_cannot_be_used_as_tool_properties() {
        let mut invalid = catalog();
        invalid.namespaces[0].name = "auto-comm".to_string();
        assert!(matches!(
            invalid.validate(),
            Err(CatalogError::InvalidName { .. })
        ));
    }
}
