use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use capability_core::{CapabilityCatalog, OperationRisk};
use capability_host::{CapabilityHost, CliArgument, CliBinding, InvocationGuard, OperationHandler};
use codex_app_server_adapter::{handle_dynamic_tool_call, is_dynamic_tool_call};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Error)]
enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Configuration(String),
    #[error("{0}")]
    Runtime(String),
}

#[derive(Debug, Clone)]
struct Options {
    catalog: PathBuf,
    bindings: PathBuf,
    codex_bin: PathBuf,
    listen: String,
    no_tui: bool,
    allow_side_effects: bool,
    tui_args: Vec<OsString>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BindingsFile {
    operations: Vec<OperationBinding>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationBinding {
    namespace: String,
    operation: String,
    command: PathBuf,
    #[serde(default)]
    args: Vec<BindingArgument>,
    working_directory: Option<PathBuf>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BindingArgument {
    Literal(String),
    Input { input: String },
}

struct RiskGuard {
    allow_side_effects: bool,
}

#[async_trait]
impl InvocationGuard for RiskGuard {
    async fn authorize(
        &self,
        namespace: &str,
        operation: &str,
        risk: OperationRisk,
        _input: &Value,
    ) -> Result<(), String> {
        if risk == OperationRisk::Read || self.allow_side_effects {
            Ok(())
        } else {
            Err(format!(
                "{namespace}.{operation} is {risk:?}; restart with --allow-side-effects after reviewing the binding"
            ))
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), CliError> {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if matches!(arguments.as_slice(), [argument] if argument == "--help" || argument == "-h")
        || matches!(arguments.as_slice(), [command, argument] if command == "run" && (argument == "--help" || argument == "-h"))
    {
        println!("{}", usage());
        return Ok(());
    }
    if matches!(arguments.as_slice(), [argument] if argument == "--version" || argument == "-V") {
        println!("codex-artifact {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let options = parse_options(arguments)?;
    let catalog = load_catalog(&options.catalog)?;
    let host = load_host(
        catalog.clone(),
        &options.bindings,
        options.allow_side_effects,
    )?;
    run_gateway(options, catalog, host).await
}

fn usage() -> String {
    "usage: codex-artifact run --catalog <capabilities.json> --bindings <bindings.json> [--codex-bin <path>] [--listen 127.0.0.1:0] [--no-tui] [--allow-side-effects] [-- <official codex TUI args>]".to_string()
}

fn parse_options(arguments: Vec<OsString>) -> Result<Options, CliError> {
    if arguments.first().and_then(|value| value.to_str()) != Some("run") {
        return Err(CliError::Usage(usage()));
    }
    let mut catalog = None;
    let mut bindings = None;
    let mut codex_bin = PathBuf::from("codex");
    let mut listen = "127.0.0.1:0".to_string();
    let mut no_tui = false;
    let mut allow_side_effects = false;
    let mut tui_args = Vec::new();
    let mut index = 1;
    while index < arguments.len() {
        let argument = arguments[index]
            .to_str()
            .ok_or_else(|| CliError::Usage("options must be valid UTF-8".to_string()))?;
        if argument == "--" {
            tui_args.extend(arguments[index + 1..].iter().cloned());
            break;
        }
        match argument {
            "--no-tui" => no_tui = true,
            "--allow-side-effects" => allow_side_effects = true,
            "--catalog" | "--bindings" | "--codex-bin" | "--listen" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| CliError::Usage(format!("{argument} requires a value")))?;
                match argument {
                    "--catalog" => catalog = Some(PathBuf::from(value)),
                    "--bindings" => bindings = Some(PathBuf::from(value)),
                    "--codex-bin" => codex_bin = PathBuf::from(value),
                    "--listen" => {
                        listen = value
                            .to_str()
                            .ok_or_else(|| {
                                CliError::Usage("--listen must be valid UTF-8".to_string())
                            })?
                            .to_string()
                    }
                    _ => unreachable!(),
                }
            }
            unknown => {
                return Err(CliError::Usage(format!(
                    "unknown option {unknown:?}\n{}",
                    usage()
                )));
            }
        }
        index += 1;
    }
    Ok(Options {
        catalog: catalog.ok_or_else(|| CliError::Usage("--catalog is required".to_string()))?,
        bindings: bindings.ok_or_else(|| CliError::Usage("--bindings is required".to_string()))?,
        codex_bin,
        listen,
        no_tui,
        allow_side_effects,
        tui_args,
    })
}

fn load_catalog(path: &Path) -> Result<CapabilityCatalog, CliError> {
    let source = std::fs::read_to_string(path).map_err(|error| {
        CliError::Configuration(format!("failed to read {}: {error}", path.display()))
    })?;
    let catalog = serde_json::from_str::<CapabilityCatalog>(&source).map_err(|error| {
        CliError::Configuration(format!("failed to parse {}: {error}", path.display()))
    })?;
    catalog
        .validate()
        .map_err(|error| CliError::Configuration(error.to_string()))?;
    Ok(catalog)
}

fn load_host(
    catalog: CapabilityCatalog,
    path: &Path,
    allow_side_effects: bool,
) -> Result<CapabilityHost, CliError> {
    let source = std::fs::read_to_string(path).map_err(|error| {
        CliError::Configuration(format!("failed to read {}: {error}", path.display()))
    })?;
    let bindings = serde_json::from_str::<BindingsFile>(&source).map_err(|error| {
        CliError::Configuration(format!("failed to parse {}: {error}", path.display()))
    })?;
    let mut handlers = BTreeMap::<(String, String), Arc<dyn OperationHandler>>::new();
    for binding in bindings.operations {
        let definition = catalog
            .operation(&binding.namespace, &binding.operation)
            .map_err(|error| CliError::Configuration(error.to_string()))?;
        if definition.kind != capability_core::OperationKind::Request {
            return Err(CliError::Configuration(format!(
                "{}.{} is a stream and cannot use a CLI request binding",
                binding.namespace, binding.operation
            )));
        }
        let key = (binding.namespace, binding.operation);
        if handlers.contains_key(&key) {
            return Err(CliError::Configuration(format!(
                "duplicate runtime binding for {}.{}",
                key.0, key.1
            )));
        }
        let arguments = binding
            .args
            .into_iter()
            .map(|argument| match argument {
                BindingArgument::Literal(value) => CliArgument::Literal(value),
                BindingArgument::Input { input } => CliArgument::InputPointer(input),
            })
            .collect();
        let mut handler = CliBinding::with_arguments(binding.command, arguments);
        handler.working_directory = binding.working_directory;
        if let Some(timeout_ms) = binding.timeout_ms {
            handler.timeout = Duration::from_millis(timeout_ms);
        }
        handlers.insert(key, Arc::new(handler));
    }
    CapabilityHost::new(
        catalog,
        Arc::new(RiskGuard { allow_side_effects }),
        handlers,
    )
    .map_err(|error| CliError::Configuration(error.to_string()))
}

async fn run_gateway(
    options: Options,
    catalog: CapabilityCatalog,
    host: CapabilityHost,
) -> Result<(), CliError> {
    let listener = TcpListener::bind(&options.listen).await.map_err(|error| {
        CliError::Runtime(format!("failed to bind {}: {error}", options.listen))
    })?;
    let address = listener
        .local_addr()
        .map_err(|error| CliError::Runtime(format!("failed to read gateway address: {error}")))?;
    if !address.ip().is_loopback() {
        return Err(CliError::Configuration(
            "the initial gateway only accepts a loopback listen address".to_string(),
        ));
    }
    let remote_url = format!("ws://{address}");
    let mut app_server = spawn_app_server(&options.codex_bin)?;
    let app_stdin = app_server
        .stdin
        .take()
        .ok_or_else(|| CliError::Runtime("Codex App Server stdin is unavailable".to_string()))?;
    let app_stdout = app_server
        .stdout
        .take()
        .ok_or_else(|| CliError::Runtime("Codex App Server stdout is unavailable".to_string()))?;
    let (app_tx, mut app_rx) = mpsc::channel::<String>(128);
    let writer = tokio::spawn(async move {
        let mut stdin = app_stdin;
        while let Some(message) = app_rx.recv().await {
            stdin.write_all(message.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
        }
        Ok::<(), std::io::Error>(())
    });

    println!("Codex Artifact Gateway: {remote_url}");
    let mut tui = if options.no_tui {
        println!("Connect the official TUI with: codex --remote {remote_url}");
        None
    } else {
        Some(spawn_tui(
            &options.codex_bin,
            &remote_url,
            &options.tui_args,
        )?)
    };

    let (stream, _) = listener
        .accept()
        .await
        .map_err(|error| CliError::Runtime(format!("failed to accept Codex TUI: {error}")))?;
    serve_connection(stream, app_stdout, app_tx.clone(), catalog, host).await?;

    drop(app_tx);
    let _ = writer.await;
    terminate(&mut app_server).await;
    if let Some(tui) = &mut tui {
        terminate(tui).await;
    }
    Ok(())
}

fn spawn_app_server(codex_bin: &Path) -> Result<Child, CliError> {
    let mut command = Command::new(codex_bin);
    command
        .arg("--enable")
        .arg("code_mode")
        .arg("--enable")
        .arg("code_mode_only")
        .arg("app-server")
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    command.spawn().map_err(|error| {
        CliError::Runtime(format!(
            "failed to start official Codex App Server {}: {error}",
            codex_bin.display()
        ))
    })
}

fn spawn_tui(
    codex_bin: &Path,
    remote_url: &str,
    arguments: &[OsString],
) -> Result<Child, CliError> {
    let mut command = Command::new(codex_bin);
    command
        .arg("--remote")
        .arg(remote_url)
        .args(arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    command.spawn().map_err(|error| {
        CliError::Runtime(format!(
            "failed to start official Codex TUI {}: {error}",
            codex_bin.display()
        ))
    })
}

async fn serve_connection(
    stream: TcpStream,
    app_stdout: tokio::process::ChildStdout,
    app_tx: mpsc::Sender<String>,
    catalog: CapabilityCatalog,
    host: CapabilityHost,
) -> Result<(), CliError> {
    let websocket = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|error| CliError::Runtime(format!("WebSocket handshake failed: {error}")))?;
    let (mut web_sink, mut web_stream) = websocket.split();
    let mut app_lines = BufReader::new(app_stdout).lines();
    loop {
        tokio::select! {
            incoming = web_stream.next() => {
                let Some(incoming) = incoming else { break };
                let message = incoming.map_err(|error| CliError::Runtime(format!("TUI WebSocket failed: {error}")))?;
                match message {
                    Message::Text(text) => {
                        let rewritten = rewrite_client_message(text.as_str(), &catalog)?;
                        app_tx.send(rewritten).await.map_err(|_| CliError::Runtime("Codex App Server input closed".to_string()))?;
                    }
                    Message::Binary(bytes) => {
                        let text = String::from_utf8(bytes.to_vec()).map_err(|error| CliError::Runtime(format!("TUI sent non-UTF-8 protocol data: {error}")))?;
                        let rewritten = rewrite_client_message(&text, &catalog)?;
                        app_tx.send(rewritten).await.map_err(|_| CliError::Runtime("Codex App Server input closed".to_string()))?;
                    }
                    Message::Ping(bytes) => web_sink.send(Message::Pong(bytes)).await.map_err(|error| CliError::Runtime(error.to_string()))?,
                    Message::Close(_) => break,
                    Message::Pong(_) | Message::Frame(_) => {}
                }
            }
            line = app_lines.next_line() => {
                let line = line.map_err(|error| CliError::Runtime(format!("failed to read Codex App Server: {error}")))?;
                let Some(line) = line else { break };
                let message = serde_json::from_str::<Value>(&line).map_err(|error| CliError::Runtime(format!("Codex App Server emitted invalid JSON: {error}")))?;
                if is_dynamic_tool_call(&message) {
                    let host = host.clone();
                    let app_tx = app_tx.clone();
                    tokio::spawn(async move {
                        let response = handle_dynamic_tool_call(&host, &message).await;
                        let value = match response {
                            Ok(value) => value,
                            Err(error) => serde_json::json!({
                                "id": message.get("id").cloned().unwrap_or(Value::Null),
                                "error": {"code": -32602, "message": error.to_string()}
                            }),
                        };
                        let _ = app_tx.send(value.to_string()).await;
                    });
                } else {
                    web_sink.send(Message::Text(line.into())).await.map_err(|error| CliError::Runtime(format!("failed to forward App Server message: {error}")))?;
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    Ok(())
}

fn rewrite_client_message(source: &str, catalog: &CapabilityCatalog) -> Result<String, CliError> {
    let mut message = serde_json::from_str::<Value>(source)
        .map_err(|error| CliError::Runtime(format!("TUI emitted invalid JSON: {error}")))?;
    match message.get("method").and_then(Value::as_str) {
        Some("initialize") => {
            let params = object_mut(&mut message, "params")?;
            if !params.contains_key("capabilities") || params["capabilities"].is_null() {
                params.insert("capabilities".to_string(), serde_json::json!({}));
            }
            let capabilities = params
                .get_mut("capabilities")
                .and_then(Value::as_object_mut)
                .ok_or_else(|| {
                    CliError::Runtime("initialize capabilities must be an object".to_string())
                })?;
            capabilities.insert("experimentalApi".to_string(), Value::Bool(true));
            capabilities
                .entry("requestAttestation".to_string())
                .or_insert(Value::Bool(false));
        }
        Some("thread/start") => {
            let tools = catalog
                .codex_dynamic_tools(true)
                .map_err(|error| CliError::Configuration(error.to_string()))?;
            object_mut(&mut message, "params")?.insert(
                "dynamicTools".to_string(),
                serde_json::to_value(tools)
                    .map_err(|error| CliError::Runtime(error.to_string()))?,
            );
        }
        _ => {}
    }
    Ok(message.to_string())
}

fn object_mut<'a>(
    message: &'a mut Value,
    field: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, CliError> {
    message
        .get_mut(field)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| CliError::Runtime(format!("protocol field {field:?} must be an object")))
}

async fn terminate(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
}

#[cfg(test)]
mod tests {
    use capability_core::{CapabilityNamespace, CapabilityOperation, OperationKind, OperationRisk};
    use serde_json::json;

    use super::*;

    fn catalog() -> CapabilityCatalog {
        CapabilityCatalog {
            schema_version: 1,
            namespaces: vec![CapabilityNamespace {
                name: "autoComm".to_string(),
                description: "Vehicle communication".to_string(),
                operations: vec![CapabilityOperation {
                    name: "readSpeed".to_string(),
                    description: "Read speed".to_string(),
                    kind: OperationKind::Request,
                    input_schema: json!({"type": "object"}),
                    output_schema: json!({"type": "object"}),
                    risk: OperationRisk::Read,
                }],
            }],
        }
    }

    #[test]
    fn enables_experimental_api_without_discarding_client_capabilities() {
        let source = json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": {"name": "codex_cli"},
                "capabilities": {"optOutNotificationMethods": ["warning"]}
            }
        })
        .to_string();
        let rewritten =
            serde_json::from_str::<Value>(&rewrite_client_message(&source, &catalog()).unwrap())
                .unwrap();
        assert_eq!(rewritten["params"]["capabilities"]["experimentalApi"], true);
        assert_eq!(
            rewritten["params"]["capabilities"]["optOutNotificationMethods"][0],
            "warning"
        );
    }

    #[test]
    fn injects_dynamic_tools_only_at_thread_start() {
        let start = rewrite_client_message(
            &json!({"method": "thread/start", "id": 2, "params": {"cwd": "/workspace"}})
                .to_string(),
            &catalog(),
        )
        .unwrap();
        let start = serde_json::from_str::<Value>(&start).unwrap();
        assert_eq!(start["params"]["dynamicTools"][0]["name"], "autoComm");

        let turn = json!({"method": "turn/start", "id": 3, "params": {}}).to_string();
        assert_eq!(rewrite_client_message(&turn, &catalog()).unwrap(), turn);
    }

    #[test]
    fn parses_official_tui_arguments_after_separator() {
        let options = parse_options(vec![
            "run".into(),
            "--catalog".into(),
            "catalog.json".into(),
            "--bindings".into(),
            "bindings.json".into(),
            "--".into(),
            "-C".into(),
            "/workspace".into(),
        ])
        .unwrap();
        assert_eq!(
            options.tui_args,
            vec![OsString::from("-C"), OsString::from("/workspace")]
        );
    }

    #[test]
    fn accepts_an_installed_codex_binary_name_by_default() {
        let options = parse_options(vec![
            "run".into(),
            "--catalog".into(),
            "catalog.json".into(),
            "--bindings".into(),
            "bindings.json".into(),
        ])
        .unwrap();
        assert_eq!(options.codex_bin, PathBuf::from("codex"));
    }
}
