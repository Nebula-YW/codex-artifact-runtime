use capability_core::{
    CapabilityCatalog, DynamicToolCallOutputContentItem, DynamicToolCallParams,
    DynamicToolCallResponse,
};
use capability_host::{CapabilityHost, Invocation};
use serde_json::{Value, json};
use thiserror::Error;

pub const ITEM_TOOL_CALL_METHOD: &str = "item/tool/call";

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("invalid App Server request: {0}")]
    InvalidRequest(String),
    #[error("failed to project capability catalog: {0}")]
    Projection(String),
}

pub fn initialize_request(id: i64, client_name: &str, client_version: &str) -> Value {
    json!({
        "method": "initialize",
        "id": id,
        "params": {
            "clientInfo": {
                "name": client_name,
                "title": "Codex Artifact Runtime",
                "version": client_version
            },
            "capabilities": {
                "experimentalApi": true,
                "requestAttestation": false,
                "optOutNotificationMethods": []
            }
        }
    })
}

pub fn initialized_notification() -> Value {
    json!({"method": "initialized", "params": {}})
}

pub fn thread_start_request(
    id: i64,
    catalog: &CapabilityCatalog,
    cwd: &str,
) -> Result<Value, AdapterError> {
    let dynamic_tools = catalog
        .codex_dynamic_tools(true)
        .map_err(|error| AdapterError::Projection(error.to_string()))?;
    Ok(json!({
        "method": "thread/start",
        "id": id,
        "params": {
            "cwd": cwd,
            "dynamicTools": dynamic_tools
        }
    }))
}

pub fn is_dynamic_tool_call(message: &Value) -> bool {
    message.get("method").and_then(Value::as_str) == Some(ITEM_TOOL_CALL_METHOD)
        && message.get("id").is_some()
        && message.get("params").is_some()
}

pub async fn handle_dynamic_tool_call(
    host: &CapabilityHost,
    request: &Value,
) -> Result<Value, AdapterError> {
    if request.get("method").and_then(Value::as_str) != Some(ITEM_TOOL_CALL_METHOD) {
        return Err(AdapterError::InvalidRequest(
            "expected item/tool/call".to_string(),
        ));
    }
    let id = request
        .get("id")
        .cloned()
        .ok_or_else(|| AdapterError::InvalidRequest("request id is missing".to_string()))?;
    let params = serde_json::from_value::<DynamicToolCallParams>(
        request.get("params").cloned().ok_or_else(|| {
            AdapterError::InvalidRequest("request params are missing".to_string())
        })?,
    )
    .map_err(|error| AdapterError::InvalidRequest(error.to_string()))?;
    let Some(namespace) = params.namespace else {
        return Ok(response(
            id,
            DynamicToolCallResponse {
                content_items: vec![DynamicToolCallOutputContentItem::InputText {
                    text: json!({"error": "top-level Dynamic Tools are not supported"}).to_string(),
                }],
                success: false,
            },
        ));
    };
    let result = host
        .invoke(Invocation {
            namespace,
            operation: params.tool,
            input: params.arguments,
        })
        .await;
    let payload = match result {
        Ok(output) => DynamicToolCallResponse {
            content_items: vec![DynamicToolCallOutputContentItem::InputText {
                text: output.to_string(),
            }],
            success: true,
        },
        Err(error) => DynamicToolCallResponse {
            content_items: vec![DynamicToolCallOutputContentItem::InputText {
                text: json!({"error": error.to_string()}).to_string(),
            }],
            success: false,
        },
    };
    Ok(response(id, payload))
}

fn response(id: Value, result: DynamicToolCallResponse) -> Value {
    json!({"id": id, "result": result})
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use capability_core::{CapabilityNamespace, CapabilityOperation, OperationKind, OperationRisk};
    use capability_host::{HostError, InvocationGuard, OperationHandler};

    use super::*;

    struct Allow;

    #[async_trait]
    impl InvocationGuard for Allow {
        async fn authorize(
            &self,
            _namespace: &str,
            _operation: &str,
            _risk: OperationRisk,
            _input: &Value,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    struct Speed;

    #[async_trait]
    impl OperationHandler for Speed {
        async fn invoke(&self, _input: Value) -> Result<Value, HostError> {
            Ok(json!({"speedKph": 50}))
        }
    }

    fn catalog() -> CapabilityCatalog {
        CapabilityCatalog {
            schema_version: 1,
            namespaces: vec![CapabilityNamespace {
                name: "autoComm".to_string(),
                description: "Vehicle communication".to_string(),
                operations: vec![CapabilityOperation {
                    name: "getSpeed".to_string(),
                    description: "Read vehicle speed".to_string(),
                    kind: OperationKind::Request,
                    input_schema: json!({"type": "object"}),
                    output_schema: json!({
                        "type": "object",
                        "properties": {"speedKph": {"type": "number"}},
                        "required": ["speedKph"]
                    }),
                    risk: OperationRisk::Read,
                }],
            }],
        }
    }

    #[test]
    fn creates_experimental_initialize_and_thread_requests() {
        assert_eq!(
            initialize_request(1, "test", "0.1.0")["params"]["capabilities"]["experimentalApi"],
            true
        );
        let request = thread_start_request(2, &catalog(), "/workspace").unwrap();
        assert_eq!(request["params"]["dynamicTools"][0]["name"], "autoComm");
        assert_eq!(
            request["params"]["dynamicTools"][0]["tools"][0]["name"],
            "getSpeed"
        );
    }

    #[tokio::test]
    async fn dispatches_item_tool_call_to_the_capability_host() {
        let handlers = BTreeMap::from([(
            ("autoComm".to_string(), "getSpeed".to_string()),
            Arc::new(Speed) as Arc<dyn OperationHandler>,
        )]);
        let host = CapabilityHost::new(catalog(), Arc::new(Allow), handlers).unwrap();
        let response = handle_dynamic_tool_call(
            &host,
            &json!({
                "method": "item/tool/call",
                "id": 42,
                "params": {
                    "threadId": "thread",
                    "turnId": "turn",
                    "callId": "call",
                    "namespace": "autoComm",
                    "tool": "getSpeed",
                    "arguments": {}
                }
            }),
        )
        .await
        .unwrap();
        assert_eq!(response["id"], 42);
        assert_eq!(response["result"]["success"], true);
        assert_eq!(
            response["result"]["contentItems"][0]["text"],
            "{\"speedKph\":50}"
        );
    }
}
