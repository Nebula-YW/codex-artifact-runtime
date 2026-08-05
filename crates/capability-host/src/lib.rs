use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use capability_core::{CapabilityCatalog, CatalogError, OperationKind, OperationRisk};
use serde_json::Value;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const DEFAULT_OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub namespace: String,
    pub operation: String,
    pub input: Value,
}

#[derive(Debug, Error)]
pub enum HostError {
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error("capability operation {namespace}.{operation} has no runtime binding")]
    MissingBinding {
        namespace: String,
        operation: String,
    },
    #[error(
        "capability operation {namespace}.{operation} is a stream and cannot use request invocation"
    )]
    StreamingOperation {
        namespace: String,
        operation: String,
    },
    #[error("capability operation {namespace}.{operation} was denied: {reason}")]
    Denied {
        namespace: String,
        operation: String,
        reason: String,
    },
    #[error("capability execution failed: {0}")]
    Execution(String),
    #[error("capability returned invalid JSON: {0}")]
    InvalidJson(String),
}

#[async_trait]
pub trait InvocationGuard: Send + Sync {
    async fn authorize(
        &self,
        namespace: &str,
        operation: &str,
        risk: OperationRisk,
        input: &Value,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait OperationHandler: Send + Sync {
    async fn invoke(&self, input: Value) -> Result<Value, HostError>;
}

#[derive(Clone)]
pub struct CapabilityHost {
    catalog: Arc<CapabilityCatalog>,
    guard: Arc<dyn InvocationGuard>,
    handlers: Arc<BTreeMap<(String, String), Arc<dyn OperationHandler>>>,
}

impl CapabilityHost {
    pub fn new(
        catalog: CapabilityCatalog,
        guard: Arc<dyn InvocationGuard>,
        handlers: BTreeMap<(String, String), Arc<dyn OperationHandler>>,
    ) -> Result<Self, HostError> {
        catalog.validate()?;
        Ok(Self {
            catalog: Arc::new(catalog),
            guard,
            handlers: Arc::new(handlers),
        })
    }

    pub fn catalog(&self) -> &CapabilityCatalog {
        &self.catalog
    }

    pub async fn invoke(&self, invocation: Invocation) -> Result<Value, HostError> {
        let operation = self
            .catalog
            .operation(&invocation.namespace, &invocation.operation)?;
        if operation.kind != OperationKind::Request {
            return Err(HostError::StreamingOperation {
                namespace: invocation.namespace,
                operation: invocation.operation,
            });
        }
        self.catalog.validate_input(
            &invocation.namespace,
            &invocation.operation,
            &invocation.input,
        )?;
        self.guard
            .authorize(
                &invocation.namespace,
                &invocation.operation,
                operation.risk,
                &invocation.input,
            )
            .await
            .map_err(|reason| HostError::Denied {
                namespace: invocation.namespace.clone(),
                operation: invocation.operation.clone(),
                reason,
            })?;
        let handler = self
            .handlers
            .get(&(invocation.namespace.clone(), invocation.operation.clone()))
            .ok_or_else(|| HostError::MissingBinding {
                namespace: invocation.namespace.clone(),
                operation: invocation.operation.clone(),
            })?;
        let output = handler.invoke(invocation.input).await?;
        self.catalog
            .validate_output(&invocation.namespace, &invocation.operation, &output)?;
        Ok(output)
    }
}

#[derive(Debug, Clone)]
pub struct CliBinding {
    pub command: PathBuf,
    pub args: Vec<CliArgument>,
    pub working_directory: Option<PathBuf>,
    pub timeout: Duration,
    pub output_limit: usize,
}

impl CliBinding {
    pub fn new(command: impl Into<PathBuf>, args: Vec<String>) -> Self {
        Self {
            command: command.into(),
            args: args.into_iter().map(CliArgument::Literal).collect(),
            working_directory: None,
            timeout: Duration::from_secs(30),
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }

    pub fn with_arguments(command: impl Into<PathBuf>, args: Vec<CliArgument>) -> Self {
        Self {
            command: command.into(),
            args,
            working_directory: None,
            timeout: Duration::from_secs(30),
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }

    fn resolve_args(&self, input: &Value) -> Result<Vec<String>, HostError> {
        self.args
            .iter()
            .map(|argument| match argument {
                CliArgument::Literal(value) => Ok(value.clone()),
                CliArgument::InputPointer(pointer) => {
                    let value = input.pointer(pointer).ok_or_else(|| {
                        HostError::Execution(format!(
                            "CLI argument input pointer {pointer:?} did not resolve"
                        ))
                    })?;
                    match value {
                        Value::String(value) => Ok(value.clone()),
                        Value::Number(value) => Ok(value.to_string()),
                        Value::Bool(value) => Ok(value.to_string()),
                        _ => Err(HostError::Execution(format!(
                            "CLI argument input pointer {pointer:?} must resolve to a scalar"
                        ))),
                    }
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub enum CliArgument {
    Literal(String),
    InputPointer(String),
}

#[async_trait]
impl OperationHandler for CliBinding {
    async fn invoke(&self, input: Value) -> Result<Value, HostError> {
        let args = self.resolve_args(&input)?;
        let mut command = Command::new(&self.command);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(directory) = &self.working_directory {
            command.current_dir(directory);
        }
        let mut child = command.spawn().map_err(|error| {
            HostError::Execution(format!(
                "failed to start {}: {error}",
                self.command.display()
            ))
        })?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| HostError::Execution("CLI stdin was not available".to_string()))?;
        let input = serde_json::to_vec(&input).map_err(|error| {
            HostError::Execution(format!("failed to encode CLI input: {error}"))
        })?;
        stdin
            .write_all(&input)
            .await
            .map_err(|error| HostError::Execution(format!("failed to write CLI input: {error}")))?;
        drop(stdin);
        let output = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| HostError::Execution(format!("CLI timed out after {:?}", self.timeout)))?
            .map_err(|error| HostError::Execution(format!("failed to wait for CLI: {error}")))?;
        if output.stdout.len() > self.output_limit || output.stderr.len() > self.output_limit {
            return Err(HostError::Execution(format!(
                "CLI output exceeded {} bytes",
                self.output_limit
            )));
        }
        if !output.status.success() {
            return Err(HostError::Execution(format!(
                "CLI exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|error| HostError::InvalidJson(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use capability_core::{CapabilityNamespace, CapabilityOperation, OperationKind, OperationRisk};
    use serde_json::json;

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

    struct Echo {
        calls: Arc<Mutex<Vec<Value>>>,
    }

    #[async_trait]
    impl OperationHandler for Echo {
        async fn invoke(&self, input: Value) -> Result<Value, HostError> {
            self.calls.lock().unwrap().push(input.clone());
            Ok(json!({"value": input["value"]}))
        }
    }

    fn catalog() -> CapabilityCatalog {
        CapabilityCatalog {
            schema_version: 1,
            namespaces: vec![CapabilityNamespace {
                name: "demo".to_string(),
                description: "Demo".to_string(),
                operations: vec![CapabilityOperation {
                    name: "echo".to_string(),
                    description: "Echo".to_string(),
                    kind: OperationKind::Request,
                    input_schema: json!({
                        "type": "object",
                        "properties": {"value": {"type": "string"}},
                        "required": ["value"]
                    }),
                    output_schema: json!({
                        "type": "object",
                        "properties": {"value": {"type": "string"}},
                        "required": ["value"]
                    }),
                    risk: OperationRisk::Read,
                }],
            }],
        }
    }

    #[tokio::test]
    async fn validates_authorizes_dispatches_and_validates_output() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let handlers = BTreeMap::from([(
            ("demo".to_string(), "echo".to_string()),
            Arc::new(Echo {
                calls: calls.clone(),
            }) as Arc<dyn OperationHandler>,
        )]);
        let host = CapabilityHost::new(catalog(), Arc::new(Allow), handlers).unwrap();
        let output = host
            .invoke(Invocation {
                namespace: "demo".to_string(),
                operation: "echo".to_string(),
                input: json!({"value": "hello"}),
            })
            .await
            .unwrap();
        assert_eq!(output, json!({"value": "hello"}));
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn rejects_invalid_input_before_dispatch() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let handlers = BTreeMap::from([(
            ("demo".to_string(), "echo".to_string()),
            Arc::new(Echo {
                calls: calls.clone(),
            }) as Arc<dyn OperationHandler>,
        )]);
        let host = CapabilityHost::new(catalog(), Arc::new(Allow), handlers).unwrap();
        assert!(
            host.invoke(Invocation {
                namespace: "demo".to_string(),
                operation: "echo".to_string(),
                input: json!({}),
            })
            .await
            .is_err()
        );
        assert!(calls.lock().unwrap().is_empty());
    }
}
