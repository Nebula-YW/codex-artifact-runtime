use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::{DEFAULT_OUTPUT_LIMIT, HostError, OperationHandler};

static NEXT_OPAQUE_ID: AtomicU64 = AtomicU64::new(1);

fn opaque_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = NEXT_OPAQUE_ID.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{millis:x}_{sequence:x}")
}

#[derive(Clone)]
pub struct SafeFileHost {
    roots: Arc<BTreeMap<String, PathBuf>>,
}

impl SafeFileHost {
    pub fn new(roots: BTreeMap<String, PathBuf>) -> Result<Self, HostError> {
        let mut canonical = BTreeMap::new();
        for (name, path) in roots {
            if name.is_empty() {
                return Err(HostError::Execution(
                    "file root name cannot be empty".into(),
                ));
            }
            fs::create_dir_all(&path).map_err(|error| {
                HostError::Execution(format!(
                    "failed to create file root {}: {error}",
                    path.display()
                ))
            })?;
            let path = fs::canonicalize(&path).map_err(|error| {
                HostError::Execution(format!(
                    "failed to resolve file root {}: {error}",
                    path.display()
                ))
            })?;
            canonical.insert(name, path);
        }
        Ok(Self {
            roots: Arc::new(canonical),
        })
    }

    fn relative_components(path: &str) -> Result<Vec<&str>, HostError> {
        if path.is_empty() || path.starts_with('/') || path.starts_with('\\') || path.contains(':')
        {
            return Err(HostError::Execution(
                "path must be a relative logical path".into(),
            ));
        }
        let components = path.split(['/', '\\']).collect::<Vec<_>>();
        if components.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || part.ends_with(' ')
                || part.ends_with('.')
                || part.contains('\0')
                || part.chars().any(|character| {
                    character < ' ' || matches!(character, '<' | '>' | '"' | '|' | '?' | '*')
                })
                || is_windows_reserved(part)
        }) {
            return Err(HostError::Execution(
                "path contains an unsafe component".into(),
            ));
        }
        Ok(components)
    }

    fn resolve_parent(&self, root: &str, path: &str) -> Result<(PathBuf, PathBuf), HostError> {
        let root_path = self
            .roots
            .get(root)
            .ok_or_else(|| HostError::Execution(format!("unknown logical file root {root:?}")))?;
        let components = Self::relative_components(path)?;
        let (file_name, directories) = components.split_last().expect("nonempty path");
        let mut parent = root_path.clone();
        for directory in directories {
            parent.push(directory);
            if parent.exists() {
                let metadata = fs::symlink_metadata(&parent).map_err(|error| {
                    HostError::Execution(format!("failed to inspect {}: {error}", parent.display()))
                })?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(HostError::Execution(
                        "path crosses a symlink or non-directory".into(),
                    ));
                }
            } else {
                fs::create_dir(&parent).map_err(|error| {
                    HostError::Execution(format!("failed to create {}: {error}", parent.display()))
                })?;
            }
        }
        let canonical_parent = fs::canonicalize(&parent).map_err(|error| {
            HostError::Execution(format!("failed to resolve {}: {error}", parent.display()))
        })?;
        if !canonical_parent.starts_with(root_path) {
            return Err(HostError::Execution("path escapes its logical root".into()));
        }
        Ok((canonical_parent.join(file_name), root_path.clone()))
    }

    fn resolve_existing_file(&self, root: &str, path: &str) -> Result<PathBuf, HostError> {
        let root_path = self
            .roots
            .get(root)
            .ok_or_else(|| HostError::Execution(format!("unknown logical file root {root:?}")))?;
        let components = Self::relative_components(path)?;
        let mut candidate = root_path.clone();
        for (index, component) in components.iter().enumerate() {
            candidate.push(component);
            let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
                HostError::Execution(format!(
                    "failed to inspect {}: {error}",
                    candidate.display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(HostError::Execution("path crosses a symlink".into()));
            }
            let is_last = index + 1 == components.len();
            if (!is_last && !metadata.is_dir()) || (is_last && !metadata.is_file()) {
                return Err(HostError::Execution(if is_last {
                    "path does not name a regular file".into()
                } else {
                    "path crosses a non-directory".into()
                }));
            }
        }
        let canonical = fs::canonicalize(&candidate).map_err(|error| {
            HostError::Execution(format!(
                "failed to resolve {}: {error}",
                candidate.display()
            ))
        })?;
        if !canonical.starts_with(root_path) {
            return Err(HostError::Execution("path escapes its logical root".into()));
        }
        Ok(canonical)
    }

    pub fn read_text(&self, root: &str, path: &str) -> Result<Value, HostError> {
        let source = self.resolve_existing_file(root, path)?;
        let metadata = fs::metadata(&source)
            .map_err(|error| HostError::Execution(format!("failed to inspect file: {error}")))?;
        if metadata.len() > DEFAULT_OUTPUT_LIMIT as u64 {
            return Err(HostError::Execution(format!(
                "text file exceeds the {} byte output limit",
                DEFAULT_OUTPUT_LIMIT
            )));
        }
        let bytes = fs::read(&source)
            .map_err(|error| HostError::Execution(format!("failed to read file: {error}")))?;
        let content = String::from_utf8(bytes)
            .map_err(|error| HostError::Execution(format!("file is not valid UTF-8: {error}")))?;
        Ok(json!({
            "root": root,
            "path": path.replace('\\', "/"),
            "bytes": content.len(),
            "content": content
        }))
    }

    pub fn write_text(&self, root: &str, path: &str, content: &str) -> Result<Value, HostError> {
        let (destination, _) = self.resolve_parent(root, path)?;
        if destination.exists() {
            return Err(HostError::Execution(format!(
                "write conflict: {} already exists",
                destination.display()
            )));
        }
        let parent = destination.parent().expect("resolved file has parent");
        let temporary = parent.join(format!(".codex-write-{}.tmp", opaque_id("file")));
        let write_result = (|| -> Result<(), HostError> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| {
                    HostError::Execution(format!("failed to create temporary file: {error}"))
                })?;
            file.write_all(content.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|error| {
                    HostError::Execution(format!("failed to write temporary file: {error}"))
                })?;
            fs::hard_link(&temporary, &destination).map_err(|error| {
                HostError::Execution(format!(
                    "failed to publish file without overwriting: {error}"
                ))
            })?;
            fs::remove_file(&temporary).map_err(|error| {
                HostError::Execution(format!("failed to remove temporary file: {error}"))
            })?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result?;
        Ok(json!({"root": root, "path": path.replace('\\', "/"), "bytes": content.len()}))
    }
}

fn is_windows_reserved(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

pub struct FileWriteTextHandler {
    host: SafeFileHost,
}

pub struct FileReadTextHandler {
    host: SafeFileHost,
}

impl FileReadTextHandler {
    pub fn new(host: SafeFileHost) -> Self {
        Self { host }
    }
}

#[async_trait]
impl OperationHandler for FileReadTextHandler {
    async fn invoke(&self, input: Value) -> Result<Value, HostError> {
        let root = required_string(&input, "root")?;
        let path = required_string(&input, "path")?;
        self.host.read_text(root, path)
    }
}

impl FileWriteTextHandler {
    pub fn new(host: SafeFileHost) -> Self {
        Self { host }
    }
}

#[async_trait]
impl OperationHandler for FileWriteTextHandler {
    async fn invoke(&self, input: Value) -> Result<Value, HostError> {
        let root = required_string(&input, "root")?;
        let path = required_string(&input, "path")?;
        let content = required_string(&input, "content")?;
        self.host.write_text(root, path, content)
    }
}

#[derive(Debug, Clone)]
pub struct AgentBrowserCliConfig {
    pub command: PathBuf,
    pub prefix_args: Vec<String>,
    pub working_directory: PathBuf,
    pub artifact_directory: PathBuf,
    pub session_name: String,
    pub profile_directory: PathBuf,
    pub executable_path: Option<PathBuf>,
    pub cdp_endpoint: Option<String>,
    pub auto_connect: bool,
    pub headed: bool,
    pub allowed_hosts: Vec<String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy)]
pub enum BrowserOperation {
    Attach,
    ListPages,
    OpenPage,
    Navigate,
    Snapshot,
    Click,
    Fill,
    Press,
    Read,
    Scroll,
    WaitFor,
    ClosePage,
    Screenshot,
    VideoStart,
    VideoStop,
    VideoInspect,
    Download,
}

#[derive(Clone)]
pub struct AgentBrowserHost {
    config: Arc<AgentBrowserCliConfig>,
    state: Arc<Mutex<BrowserState>>,
}

#[derive(Default)]
struct BrowserState {
    sessions: BTreeMap<String, BrowserSession>,
    videos: BTreeMap<String, Artifact>,
}

struct BrowserSession {
    cli_name: String,
    pages: BTreeMap<String, String>,
    video: Option<Artifact>,
    video_starting: bool,
    video_stopping: bool,
}

#[derive(Clone)]
struct Artifact {
    id: String,
    absolute_path: PathBuf,
    relative_path: String,
}

#[derive(Debug)]
struct PageInfo {
    tab_id: String,
    title: String,
    url: String,
    current: bool,
}

impl AgentBrowserHost {
    pub fn new(mut config: AgentBrowserCliConfig) -> Result<Self, HostError> {
        if config.session_name.is_empty() || config.session_name.contains(char::is_whitespace) {
            return Err(HostError::Execution(
                "agent-browser session name is invalid".into(),
            ));
        }
        if config.auto_connect && config.cdp_endpoint.is_some() {
            return Err(HostError::Execution(
                "agent-browser auto-connect and CDP endpoint are mutually exclusive".into(),
            ));
        }
        if config
            .cdp_endpoint
            .as_deref()
            .is_some_and(|endpoint| endpoint.trim().is_empty())
        {
            return Err(HostError::Execution(
                "agent-browser CDP endpoint is empty".into(),
            ));
        }
        if config.artifact_directory.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(HostError::Execution(
                "artifact directory must be relative".into(),
            ));
        }
        config.command = resolve_agent_browser_command(&config.command)?;
        fs::create_dir_all(&config.working_directory)
            .map_err(io_error("create working directory"))?;
        config.working_directory = fs::canonicalize(&config.working_directory)
            .map_err(io_error("resolve working directory"))?;
        if !config.profile_directory.is_absolute() {
            config.profile_directory = config.working_directory.join(&config.profile_directory);
        }
        fs::create_dir_all(&config.profile_directory)
            .map_err(io_error("create browser profile directory"))?;
        config.profile_directory = fs::canonicalize(&config.profile_directory)
            .map_err(io_error("resolve browser profile directory"))?;
        let artifact = config.working_directory.join(&config.artifact_directory);
        fs::create_dir_all(&artifact).map_err(io_error("create artifact directory"))?;
        let artifact =
            fs::canonicalize(&artifact).map_err(io_error("resolve artifact directory"))?;
        if !artifact.starts_with(&config.working_directory) {
            return Err(HostError::Execution(
                "artifact directory escapes working directory".into(),
            ));
        }
        config.artifact_directory = artifact;
        Ok(Self {
            config: Arc::new(config),
            state: Arc::new(Mutex::new(BrowserState::default())),
        })
    }

    async fn run_cli(
        &self,
        session: Option<&str>,
        command_args: Vec<String>,
    ) -> Result<Value, HostError> {
        let mut args = self.config.prefix_args.clone();
        if let Some(session) = session {
            args.extend(["--session".into(), session.into()]);
        }
        if let Some(endpoint) = &self.config.cdp_endpoint {
            args.extend(["--cdp".into(), endpoint.clone()]);
        } else if self.config.auto_connect {
            args.push("--auto-connect".into());
        } else {
            args.extend([
                "--profile".into(),
                self.config.profile_directory.to_string_lossy().into_owned(),
            ]);
            if self.config.headed {
                args.push("--headed".into());
            }
            if let Some(executable) = &self.config.executable_path {
                args.extend([
                    "--executable-path".into(),
                    executable.to_string_lossy().into_owned(),
                ]);
            }
        }
        args.extend([
            "--json".into(),
            "--max-output".into(),
            DEFAULT_OUTPUT_LIMIT.to_string(),
        ]);
        args.extend(command_args);
        // agent-browser starts a long-lived daemon. On Windows that daemon can
        // inherit anonymous stdout/stderr pipes, keeping `Command::output()`
        // waiting for EOF after the short-lived CLI client has exited. Regular
        // files do not have that EOF coupling, so capture each invocation in
        // bounded temporary files and wait only for the direct child process.
        let invocation = opaque_id("agent-browser-call");
        let stdout_path = self
            .config
            .artifact_directory
            .join(format!(".{invocation}.stdout"));
        let stderr_path = self
            .config
            .artifact_directory
            .join(format!(".{invocation}.stderr"));
        let stdout_file = fs::File::create(&stdout_path)
            .map_err(io_error("create agent-browser stdout capture"))?;
        let stderr_file = fs::File::create(&stderr_path)
            .map_err(io_error("create agent-browser stderr capture"))?;
        let mut command = Command::new(&self.config.command);
        command
            .args(&args)
            .current_dir(&self.config.working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file))
            .kill_on_drop(true);
        for (name, _) in std::env::vars_os() {
            if name
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("AGENT_BROWSER_")
            {
                command.env_remove(name);
            }
        }
        let mut child = command.spawn().map_err(|error| {
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            HostError::Execution(format!(
                "failed to start {}: {error}",
                self.config.command.display()
            ))
        })?;
        let status = match tokio::time::timeout(self.config.timeout, child.wait()).await {
            Ok(result) => result.map_err(|error| {
                HostError::Execution(format!("failed to wait for agent-browser: {error}"))
            })?,
            Err(_) => {
                let _ = child.kill().await;
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                return Err(HostError::Execution(format!(
                    "agent-browser timed out after {:?}",
                    self.config.timeout
                )));
            }
        };
        let stdout_size = fs::metadata(&stdout_path)
            .map(|value| value.len())
            .unwrap_or(0);
        let stderr_size = fs::metadata(&stderr_path)
            .map(|value| value.len())
            .unwrap_or(0);
        if stdout_size > DEFAULT_OUTPUT_LIMIT as u64 || stderr_size > DEFAULT_OUTPUT_LIMIT as u64 {
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Err(HostError::Execution(
                "agent-browser output exceeded 8 MiB".into(),
            ));
        }
        let stdout = fs::read(&stdout_path).map_err(io_error("read agent-browser stdout"))?;
        let stderr = fs::read(&stderr_path).map_err(io_error("read agent-browser stderr"))?;
        let _ = fs::remove_file(&stdout_path);
        let _ = fs::remove_file(&stderr_path);
        if !status.success() {
            let stdout_text = String::from_utf8_lossy(&stdout);
            let stderr_text = String::from_utf8_lossy(&stderr);
            let detail = if stderr_text.trim().is_empty() {
                stdout_text.trim()
            } else if stdout_text.trim().is_empty() {
                stderr_text.trim()
            } else {
                return Err(HostError::Execution(format!(
                    "agent-browser exited with {status}: stderr={}; stdout={}",
                    stderr_text.trim(),
                    stdout_text.trim()
                )));
            };
            return Err(HostError::Execution(format!(
                "agent-browser exited with {}: {}",
                status, detail
            )));
        }
        if stdout.iter().all(u8::is_ascii_whitespace) {
            return Err(HostError::Execution(format!(
                "agent-browser returned no JSON: {}",
                String::from_utf8_lossy(&stderr).trim()
            )));
        }
        let value: Value = serde_json::from_slice(&stdout).map_err(|error| {
            HostError::InvalidJson(format!(
                "agent-browser: {error}; stdout={}",
                String::from_utf8_lossy(&stdout)
            ))
        })?;
        if value.get("isError").and_then(Value::as_bool) == Some(true)
            || value.get("success").and_then(Value::as_bool) == Some(false)
        {
            return Err(HostError::Execution(result_text(&value)));
        }
        Ok(value)
    }

    async fn attach(&self) -> Result<Value, HostError> {
        let command = if self.config.auto_connect || self.config.cdp_endpoint.is_some() {
            vec!["get".into(), "url".into()]
        } else {
            vec!["open".into(), "about:blank".into()]
        };
        self.run_cli(Some(&self.config.session_name), command)
            .await?;
        let session_id = opaque_id("session");
        self.state.lock().await.sessions.insert(
            session_id.clone(),
            BrowserSession {
                cli_name: self.config.session_name.clone(),
                pages: BTreeMap::new(),
                video: None,
                video_starting: false,
                video_stopping: false,
            },
        );
        Ok(json!({"session_id": session_id, "browser": "agent-browser", "attached": true}))
    }

    async fn session_name(&self, session_id: &str) -> Result<String, HostError> {
        self.state
            .lock()
            .await
            .sessions
            .get(session_id)
            .map(|session| session.cli_name.clone())
            .ok_or_else(|| HostError::Execution("unknown or expired session_id".into()))
    }

    async fn refresh_pages(&self, session_id: &str) -> Result<Vec<(String, PageInfo)>, HostError> {
        let name = self.session_name(session_id).await?;
        let value = self.run_cli(Some(&name), vec!["tab".into()]).await?;
        let parsed = parse_pages(&value);
        let mut state = self.state.lock().await;
        let session = state
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| HostError::Execution("unknown or expired session_id".into()))?;
        let mut pages = Vec::new();
        for page in parsed {
            let page_id = session
                .pages
                .iter()
                .find_map(|(id, tab_id)| (*tab_id == page.tab_id).then(|| id.clone()))
                .unwrap_or_else(|| opaque_id("page"));
            session.pages.insert(page_id.clone(), page.tab_id.clone());
            pages.push((page_id, page));
        }
        let tab_ids = pages
            .iter()
            .map(|(_, page)| page.tab_id.clone())
            .collect::<Vec<_>>();
        session.pages.retain(|_, tab_id| tab_ids.contains(tab_id));
        Ok(pages)
    }

    async fn list_pages(&self, session_id: &str) -> Result<Value, HostError> {
        let pages = self.refresh_pages(session_id).await?;
        Ok(self.pages_value(pages))
    }

    fn pages_value(&self, pages: Vec<(String, PageInfo)>) -> Value {
        json!({"pages": pages.into_iter().filter_map(|(page_id, page)| {
            self.ensure_url_allowed(&page.url).ok().map(|_| json!({
                "page_id": page_id, "title": page.title, "url": page.url, "current": page.current
            }))
        }).collect::<Vec<_>>() })
    }

    async fn select_page(
        &self,
        session_id: &str,
        page_id: &str,
    ) -> Result<(String, String), HostError> {
        let pages = self.refresh_pages(session_id).await?;
        let (_, page) = pages
            .iter()
            .find(|(id, _)| id == page_id)
            .ok_or_else(|| HostError::Execution("unknown or expired page_id".into()))?;
        self.ensure_url_allowed(&page.url)?;
        let name = self.session_name(session_id).await?;
        // Snapshot refs belong to the latest page state. Avoid switching an already
        // active tab so a snapshot ref remains usable by the next operation.
        if !page.current {
            self.run_cli(Some(&name), vec!["tab".into(), page.tab_id.clone()])
                .await?;
        }
        Ok((name, page.tab_id.clone()))
    }

    fn ensure_url_allowed(&self, url: &str) -> Result<(), HostError> {
        let host = http_host(url)
            .ok_or_else(|| HostError::Execution("page URL is not an allowed HTTP(S) URL".into()))?;
        let allowed = self.config.allowed_hosts.iter().any(|candidate| {
            let candidate = candidate.to_ascii_lowercase();
            if let Some(suffix) = candidate.strip_prefix("*.") {
                host.ends_with(&format!(".{suffix}")) && host != suffix
            } else {
                host == candidate
            }
        });
        if allowed {
            Ok(())
        } else {
            Err(HostError::Execution(format!(
                "host {host:?} is outside the browser allowlist"
            )))
        }
    }

    fn new_artifact(&self, extension: &str) -> Artifact {
        let id = opaque_id("artifact");
        let file_name = format!("{id}.{extension}");
        Artifact {
            id,
            absolute_path: self.config.artifact_directory.join(&file_name),
            relative_path: self
                .config
                .artifact_directory
                .strip_prefix(&self.config.working_directory)
                .unwrap_or(Path::new("artifacts/browser"))
                .join(file_name)
                .to_string_lossy()
                .replace('\\', "/"),
        }
    }

    async fn invoke_operation(
        &self,
        operation: BrowserOperation,
        input: Value,
    ) -> Result<Value, HostError> {
        if matches!(operation, BrowserOperation::Attach) {
            return self.attach().await;
        }
        let session_id = required_string(&input, "session_id")?;
        if matches!(operation, BrowserOperation::ListPages) {
            return self.list_pages(session_id).await;
        }
        if matches!(operation, BrowserOperation::OpenPage) {
            let url = required_string(&input, "url")?;
            self.ensure_url_allowed(url)?;
            let name = self.session_name(session_id).await?;
            self.run_cli(Some(&name), vec!["tab".into(), "new".into(), url.into()])
                .await?;
            let pages = self.refresh_pages(session_id).await?;
            let current = pages
                .iter()
                .find(|(_, page)| page.current)
                .ok_or_else(|| HostError::Execution("new page was not found".into()))?;
            self.ensure_url_allowed(&current.1.url)?;
            return Ok(self.pages_value(pages));
        }
        let page_id = required_string(&input, "page_id")?;
        let (name, tab_id) = self.select_page(session_id, page_id).await?;
        match operation {
            BrowserOperation::Navigate => {
                let url = required_string(&input, "url")?;
                self.ensure_url_allowed(url)?;
                self.run_cli(Some(&name), vec!["open".into(), url.into()])
                    .await?;
                let pages = self.refresh_pages(session_id).await?;
                let current = pages
                    .iter()
                    .find(|(_, page)| page.current)
                    .ok_or_else(|| HostError::Execution("navigated page was not found".into()))?;
                self.ensure_url_allowed(&current.1.url)?;
                Ok(self.pages_value(pages))
            }
            BrowserOperation::Snapshot => {
                // Interactive-only snapshots keep large application pages bounded while
                // preserving the stable refs used by click, fill, and read operations.
                let value = self
                    .run_cli(Some(&name), vec!["snapshot".into(), "-i".into()])
                    .await?;
                Ok(json!({"snapshot": result_text(&value)}))
            }
            BrowserOperation::Click => {
                let target = safe_target(&input)?;
                let value = self
                    .run_cli(Some(&name), vec!["click".into(), target.into()])
                    .await?;
                Ok(json!({"result": result_text(&value)}))
            }
            BrowserOperation::Fill => {
                let target = safe_target(&input)?;
                let text = required_string(&input, "text")?;
                let value = self
                    .run_cli(Some(&name), vec!["fill".into(), target, text.into()])
                    .await?;
                Ok(json!({"result": result_text(&value)}))
            }
            BrowserOperation::Press => {
                let key = safe_key(&input)?;
                let value = self
                    .run_cli(Some(&name), vec!["press".into(), key.into()])
                    .await?;
                Ok(json!({"result": result_text(&value)}))
            }
            BrowserOperation::Read => {
                let target = safe_target(&input)?;
                let value = self
                    .run_cli(Some(&name), vec!["get".into(), "text".into(), target])
                    .await?;
                Ok(json!({"text": result_text(&value)}))
            }
            BrowserOperation::Scroll => {
                let delta_x = input
                    .get("delta_x")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .clamp(-10000, 10000);
                let delta_y = input
                    .get("delta_y")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .clamp(-10000, 10000);
                self.run_cli(
                    Some(&name),
                    vec![
                        "mouse".into(),
                        "wheel".into(),
                        delta_y.to_string(),
                        delta_x.to_string(),
                    ],
                )
                .await?;
                Ok(json!({"scrolled": true}))
            }
            BrowserOperation::WaitFor => {
                let text = required_string(&input, "text")?;
                let value = self
                    .run_cli(
                        Some(&name),
                        vec!["wait".into(), "--text".into(), text.into()],
                    )
                    .await?;
                Ok(json!({"result": result_text(&value)}))
            }
            BrowserOperation::ClosePage => {
                self.run_cli(Some(&name), vec!["tab".into(), "close".into(), tab_id])
                    .await?;
                Ok(json!({"closed": true}))
            }
            BrowserOperation::Screenshot => {
                let artifact = self.new_artifact("png");
                self.run_cli(
                    Some(&name),
                    vec![
                        "screenshot".into(),
                        artifact.absolute_path.to_string_lossy().into_owned(),
                    ],
                )
                .await?;
                Ok(artifact_value(&artifact))
            }
            BrowserOperation::VideoStart => {
                {
                    let mut state = self.state.lock().await;
                    let session = state.sessions.get_mut(session_id).ok_or_else(|| {
                        HostError::Execution("unknown or expired session_id".into())
                    })?;
                    if session.video.is_some() || session.video_starting {
                        return Err(HostError::Execution(
                            "a video is already recording for this session".into(),
                        ));
                    }
                    session.video_starting = true;
                }
                let artifact = self.new_artifact("webm");
                if let Err(error) = self
                    .run_cli(
                        Some(&name),
                        vec![
                            "record".into(),
                            "start".into(),
                            artifact.absolute_path.to_string_lossy().into_owned(),
                        ],
                    )
                    .await
                {
                    if let Some(session) = self.state.lock().await.sessions.get_mut(session_id) {
                        session.video_starting = false;
                    }
                    return Err(error);
                }
                let mut state = self.state.lock().await;
                let session = state
                    .sessions
                    .get_mut(session_id)
                    .ok_or_else(|| HostError::Execution("unknown or expired session_id".into()))?;
                session.video_starting = false;
                session.video = Some(artifact.clone());
                Ok(json!({"recording": true, "artifact_id": artifact.id}))
            }
            BrowserOperation::VideoStop | BrowserOperation::VideoInspect => {
                unreachable!("handled before page selection")
            }
            BrowserOperation::Download => {
                let target = safe_target(&input)?;
                let artifact = self.new_artifact("download");
                let value = self
                    .run_cli(
                        Some(&name),
                        vec![
                            "download".into(),
                            target,
                            artifact.absolute_path.to_string_lossy().into_owned(),
                        ],
                    )
                    .await?;
                Ok(
                    json!({"artifact_id": artifact.id, "root": "workspace", "path": artifact.relative_path, "suggested_filename": result_text(&value)}),
                )
            }
            BrowserOperation::Attach | BrowserOperation::ListPages | BrowserOperation::OpenPage => {
                unreachable!()
            }
        }
    }

    async fn stop_video(&self, session_id: &str) -> Result<Value, HostError> {
        {
            let mut state = self.state.lock().await;
            let session = state
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| HostError::Execution("unknown or expired session_id".into()))?;
            if session.video.is_none() || session.video_stopping {
                return Err(HostError::Execution("no video recording is active".into()));
            }
            session.video_stopping = true;
        }
        let name = self.session_name(session_id).await?;
        if let Err(error) = self
            .run_cli(Some(&name), vec!["record".into(), "stop".into()])
            .await
        {
            if let Some(session) = self.state.lock().await.sessions.get_mut(session_id) {
                session.video_stopping = false;
            }
            return Err(error);
        }
        let artifact = {
            let mut state = self.state.lock().await;
            let session = state
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| HostError::Execution("unknown or expired session_id".into()))?;
            session.video_stopping = false;
            session
                .video
                .take()
                .ok_or_else(|| HostError::Execution("no video recording is active".into()))?
        };
        validate_webm(&artifact.absolute_path)?;
        self.state
            .lock()
            .await
            .videos
            .insert(artifact.id.clone(), artifact.clone());
        Ok(artifact_value(&artifact))
    }

    async fn inspect_video(&self, artifact_id: &str) -> Result<Value, HostError> {
        let artifact = self
            .state
            .lock()
            .await
            .videos
            .get(artifact_id)
            .cloned()
            .ok_or_else(|| HostError::Execution("unknown or expired video artifact_id".into()))?;
        validate_webm(&artifact.absolute_path)?;

        let probe = self
            .run_media_tool(
                "ffprobe",
                vec![
                    "-v".into(),
                    "error".into(),
                    "-select_streams".into(),
                    "v:0".into(),
                    "-show_entries".into(),
                    "stream=codec_name:format=format_name,duration".into(),
                    "-of".into(),
                    "json".into(),
                    artifact.absolute_path.as_os_str().to_owned(),
                ],
            )
            .await?;
        let (container, codec, duration_seconds) = parse_video_probe(&probe)?;

        self.run_media_tool(
            "ffmpeg",
            vec![
                "-v".into(),
                "error".into(),
                "-nostdin".into(),
                "-i".into(),
                artifact.absolute_path.as_os_str().to_owned(),
                "-map".into(),
                "0:v:0".into(),
                "-f".into(),
                "null".into(),
                "-".into(),
            ],
        )
        .await?;

        let mut hashes = Vec::new();
        for position in sample_positions(duration_seconds) {
            let output = self
                .run_media_tool(
                    "ffmpeg",
                    vec![
                        "-v".into(),
                        "error".into(),
                        "-nostdin".into(),
                        "-ss".into(),
                        format!("{position:.6}").into(),
                        "-i".into(),
                        artifact.absolute_path.as_os_str().to_owned(),
                        "-map".into(),
                        "0:v:0".into(),
                        "-frames:v".into(),
                        "1".into(),
                        "-f".into(),
                        "hash".into(),
                        "-hash".into(),
                        "sha256".into(),
                        "-".into(),
                    ],
                )
                .await?;
            hashes.push(parse_frame_hash(&output)?);
        }
        let mut distinct = hashes.clone();
        distinct.sort_unstable();
        distinct.dedup();

        Ok(json!({
            "artifact_id": artifact.id,
            "container": container,
            "video_codec": codec,
            "decodable": true,
            "duration_seconds": duration_seconds,
            "sampled_frames": hashes.len(),
            "distinct_frame_hashes": distinct.len(),
            "frames_changed": distinct.len() >= 2
        }))
    }

    async fn run_media_tool(
        &self,
        executable: &'static str,
        args: Vec<OsString>,
    ) -> Result<Vec<u8>, HostError> {
        let mut command = Command::new(executable);
        command
            .args(args)
            .current_dir(&self.config.working_directory)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let output = tokio::time::timeout(self.config.timeout, command.output())
            .await
            .map_err(|_| {
                HostError::Execution(format!(
                    "{executable} timed out after {:?}",
                    self.config.timeout
                ))
            })?
            .map_err(|error| {
                HostError::Execution(format!("failed to start {executable}: {error}"))
            })?;
        if output.stdout.len() > DEFAULT_OUTPUT_LIMIT || output.stderr.len() > DEFAULT_OUTPUT_LIMIT
        {
            return Err(HostError::Execution(format!(
                "{executable} output exceeded 8 MiB"
            )));
        }
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            return Err(HostError::Execution(format!(
                "{executable} exited with {}: {}",
                output.status,
                detail.trim()
            )));
        }
        Ok(output.stdout)
    }
}

fn resolve_agent_browser_command(command: &Path) -> Result<PathBuf, HostError> {
    #[cfg(windows)]
    {
        let file_name = command
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let npm_shim = matches!(
            file_name.as_str(),
            "agent-browser" | "agent-browser.cmd" | "agent-browser.ps1"
        );
        if npm_shim {
            let mut roots = Vec::new();
            if command.components().count() > 1 {
                if let Some(parent) = command.parent() {
                    roots.push(parent.to_path_buf());
                }
            } else if let Some(path) = std::env::var_os("PATH") {
                roots.extend(std::env::split_paths(&path));
            }
            for root in roots {
                let native = root
                    .join("node_modules")
                    .join("agent-browser")
                    .join("bin")
                    .join("agent-browser-win32-x64.exe");
                if native.is_file() {
                    return fs::canonicalize(&native)
                        .map_err(io_error("resolve native agent-browser executable"));
                }
            }
            return Err(HostError::Execution(
                "agent-browser npm shim was found, but its native Windows executable was not; reinstall agent-browser or configure command with the native .exe path".into(),
            ));
        }
        if matches!(
            command.extension().and_then(|value| value.to_str()),
            Some("cmd" | "bat" | "ps1")
        ) {
            return Err(HostError::Execution(
                "agent-browser command must be a native executable, not a shell script".into(),
            ));
        }
    }
    Ok(command.to_path_buf())
}

pub struct BrowserOperationHandler {
    host: AgentBrowserHost,
    operation: BrowserOperation,
}

impl BrowserOperationHandler {
    pub fn new(host: AgentBrowserHost, operation: BrowserOperation) -> Self {
        Self { host, operation }
    }
}

#[async_trait]
impl OperationHandler for BrowserOperationHandler {
    async fn invoke(&self, input: Value) -> Result<Value, HostError> {
        if matches!(self.operation, BrowserOperation::VideoStop) {
            let session_id = required_string(&input, "session_id")?;
            self.host.stop_video(session_id).await
        } else if matches!(self.operation, BrowserOperation::VideoInspect) {
            let artifact_id = required_string(&input, "artifact_id")?;
            self.host.inspect_video(artifact_id).await
        } else {
            self.host.invoke_operation(self.operation, input).await
        }
    }
}

fn required_string<'a>(input: &'a Value, field: &str) -> Result<&'a str, HostError> {
    input
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            HostError::Execution(format!("input field {field:?} must be a nonempty string"))
        })
}

fn safe_target(input: &Value) -> Result<String, HostError> {
    let target = required_string(input, "target")?;
    let element = target
        .strip_prefix('@')
        .unwrap_or(target)
        .strip_prefix('e')
        .filter(|digits| !digits.is_empty());
    if element.is_some_and(|digits| digits.chars().all(|character| character.is_ascii_digit())) {
        Ok(format!("@{}", target.strip_prefix('@').unwrap_or(target)))
    } else {
        Err(HostError::Execution(
            "target must be an agent-browser element ref such as e12 or @e12 from the latest snapshot".into(),
        ))
    }
}

fn safe_key(input: &Value) -> Result<&str, HostError> {
    let key = required_string(input, "key")?;
    if key.len() <= 48
        && key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '_' | '.')
        })
    {
        Ok(key)
    } else {
        Err(HostError::Execution(
            "key contains unsupported characters".into(),
        ))
    }
}

fn result_text(value: &Value) -> String {
    let text = value
        .pointer("/data/text")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/data/snapshot").and_then(Value::as_str))
        .or_else(|| value.pointer("/data/message").and_then(Value::as_str))
        .or_else(|| value.get("data").and_then(Value::as_str))
        .or_else(|| value.get("result").and_then(Value::as_str))
        .or_else(|| value.get("message").and_then(Value::as_str))
        .unwrap_or("ok");
    text.to_string()
}

fn parse_pages(value: &Value) -> Vec<PageInfo> {
    let tabs = value
        .pointer("/data/tabs")
        .and_then(Value::as_array)
        .or_else(|| value.get("tabs").and_then(Value::as_array))
        .or_else(|| value.get("data").and_then(Value::as_array));
    tabs.into_iter()
        .flatten()
        .filter_map(|tab| {
            let tab_id = tab
                .get("id")
                .or_else(|| tab.get("tabId"))
                .or_else(|| tab.get("targetId"))?
                .as_str()?
                .to_string();
            let url = tab.get("url")?.as_str()?.to_string();
            let title = tab
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let current = tab
                .get("active")
                .or_else(|| tab.get("current"))
                .or_else(|| tab.get("isActive"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(PageInfo {
                tab_id,
                title,
                url,
                current,
            })
        })
        .collect()
}

fn http_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    if let Some(ipv6) = authority.strip_prefix('[') {
        return Some(ipv6.split(']').next()?.to_ascii_lowercase());
    }
    Some(
        authority
            .split(':')
            .next()?
            .trim_end_matches('.')
            .to_ascii_lowercase(),
    )
}

fn artifact_value(artifact: &Artifact) -> Value {
    json!({"artifact_id": artifact.id, "root": "workspace", "path": artifact.relative_path})
}

fn validate_webm(path: &Path) -> Result<(), HostError> {
    let bytes = fs::read(path).map_err(io_error("read recorded video"))?;
    if bytes.len() < 4 || bytes[..4] != [0x1a, 0x45, 0xdf, 0xa3] {
        let _ = fs::remove_file(path);
        return Err(HostError::Execution(
            "agent-browser produced an invalid WebM recording; the incomplete file was removed"
                .into(),
        ));
    }
    Ok(())
}

fn parse_video_probe(output: &[u8]) -> Result<(String, String, f64), HostError> {
    let value: Value = serde_json::from_slice(output)
        .map_err(|error| HostError::InvalidJson(format!("ffprobe: {error}")))?;
    let container = value
        .pointer("/format/format_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| HostError::Execution("ffprobe omitted the container format".into()))?;
    let codec = value
        .pointer("/streams/0/codec_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| HostError::Execution("ffprobe found no video stream".into()))?;
    let duration = value
        .pointer("/format/duration")
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| {
            HostError::Execution("ffprobe returned no positive finite duration".into())
        })?;
    Ok((container.into(), codec.into(), duration))
}

fn sample_positions(duration_seconds: f64) -> [f64; 3] {
    [0.1, 0.5, 0.9].map(|fraction| duration_seconds * fraction)
}

fn parse_frame_hash(output: &[u8]) -> Result<String, HostError> {
    let text = std::str::from_utf8(output)
        .map_err(|error| HostError::Execution(format!("ffmpeg hash was not UTF-8: {error}")))?;
    let hash = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("SHA256="))
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| HostError::Execution("ffmpeg returned no SHA-256 frame hash".into()))?;
    Ok(hash.to_ascii_lowercase())
}

fn io_error(action: &'static str) -> impl Fn(std::io::Error) -> HostError {
    move |error| HostError::Execution(format!("failed to {action}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_ref(snapshot: &str, candidates: &[&str]) -> Result<String, HostError> {
        for candidate in candidates {
            if let Some(reference) = snapshot.lines().find_map(|line| {
                if !line.contains(candidate) {
                    return None;
                }
                line.split("[ref=")
                    .nth(1)
                    .and_then(|tail| tail.split(']').next())
                    .map(str::to_owned)
            }) {
                return Ok(reference);
            }
        }
        Err(HostError::Execution(format!(
            "none of the expected elements {candidates:?} appeared in snapshot:\n{snapshot}"
        )))
    }

    async fn smoke_snapshot(
        host: &AgentBrowserHost,
        session_id: &str,
        page_id: &str,
    ) -> Result<String, HostError> {
        let value = host
            .invoke_operation(
                BrowserOperation::Snapshot,
                json!({"session_id": session_id, "page_id": page_id}),
            )
            .await?;
        value["snapshot"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| HostError::Execution("snapshot response omitted text".into()))
    }

    #[test]
    fn rejects_traversal_absolute_ads_and_reserved_paths() {
        for path in [
            "../x",
            "/x",
            "C:/x",
            "x:y",
            "dir/../x",
            "CON.txt",
            "dir/NUL",
            "bad?.txt",
            "bad|name.txt",
            "control\u{1f}.txt",
        ] {
            assert!(
                SafeFileHost::relative_components(path).is_err(),
                "accepted {path}"
            );
        }
    }

    #[test]
    fn writes_once_without_overwriting_existing_content() {
        let directory = std::env::temp_dir().join(opaque_id("safe-file-test"));
        fs::create_dir(&directory).expect("create isolated test directory");
        let host = SafeFileHost::new(BTreeMap::from([("workspace".into(), directory.clone())]))
            .expect("create safe file host");

        let written = host
            .write_text("workspace", "nested/result.txt", "first")
            .expect("write new file");
        assert_eq!(written["bytes"], 5);
        assert_eq!(
            fs::read_to_string(directory.join("nested/result.txt")).expect("read written file"),
            "first"
        );
        assert!(
            host.write_text("workspace", "nested/result.txt", "second")
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(directory.join("nested/result.txt")).expect("read original file"),
            "first"
        );

        fs::remove_dir_all(&directory).expect("remove isolated test directory");
    }

    #[test]
    fn reads_utf8_text_without_crossing_the_logical_root() {
        let directory = std::env::temp_dir().join(opaque_id("safe-file-read-test"));
        fs::create_dir(&directory).expect("create isolated test directory");
        fs::create_dir(directory.join("docs")).expect("create docs directory");
        fs::write(directory.join("docs/runbook.md"), "hello 世界").expect("write fixture");
        let host = SafeFileHost::new(BTreeMap::from([("workspace".into(), directory.clone())]))
            .expect("create safe file host");

        let read = host
            .read_text("workspace", "docs/runbook.md")
            .expect("read text file");
        assert_eq!(read["content"], "hello 世界");
        assert_eq!(read["bytes"], "hello 世界".len());
        assert!(host.read_text("workspace", "../outside.txt").is_err());

        fs::remove_dir_all(&directory).expect("remove isolated test directory");
    }

    #[test]
    fn parses_agent_browser_tabs_without_exposing_cli_ids() {
        let pages = parse_pages(&json!({
            "success": true,
            "data": {"tabs": [
                {"id":"t1","title":"Home","url":"https://example.com/","active":true},
                {"id":"t2","title":"Docs","url":"https://example.com/docs","active":false}
            ]}
        }));
        assert_eq!(pages.len(), 2);
        assert!(pages[0].current);
        assert_eq!(pages[0].tab_id, "t1");
        assert_eq!(pages[1].url, "https://example.com/docs");
    }

    #[test]
    fn parses_http_hosts_strictly() {
        assert_eq!(
            http_host("https://Example.COM:443/a"),
            Some("example.com".into())
        );
        assert_eq!(http_host("file:///secret"), None);
        assert_eq!(http_host("https://user@example.com"), None);
    }

    #[test]
    fn accepts_agent_browser_element_refs_only() {
        for target in ["e12", "@e199"] {
            assert_eq!(
                safe_target(&json!({"target": target})).unwrap(),
                format!("@{}", target.trim_start_matches('@'))
            );
        }
        for target in ["e", "f2e", "fe1", "button", "e1;alert(1)"] {
            assert!(
                safe_target(&json!({"target": target})).is_err(),
                "accepted {target}"
            );
        }
    }

    #[test]
    fn parses_video_probe_and_frame_hash_output() {
        let probe = br#"{
            "streams": [{"codec_name": "vp9"}],
            "format": {"format_name": "matroska,webm", "duration": "9.143000"}
        }"#;
        let (container, codec, duration) = parse_video_probe(probe).expect("parse ffprobe JSON");
        assert_eq!(container, "matroska,webm");
        assert_eq!(codec, "vp9");
        assert!((duration - 9.143).abs() < f64::EPSILON);
        for (actual, expected) in sample_positions(duration)
            .into_iter()
            .zip([0.9143, 4.5715, 8.2287])
        {
            assert!((actual - expected).abs() < 1e-12);
        }

        let hash = parse_frame_hash(
            b"SHA256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789ABCDEF\n",
        )
        .expect("parse ffmpeg hash");
        assert_eq!(
            hash,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn rejects_incomplete_video_probe_and_frame_hash_output() {
        assert!(parse_video_probe(br#"{"streams": [], "format": {}}"#).is_err());
        assert!(parse_frame_hash(b"MD5=abc\n").is_err());
    }

    #[tokio::test]
    #[ignore = "requires FFprobe/FFmpeg and WEBM_INSPECTION_FIXTURE"]
    async fn inspects_a_registered_webm_fixture() {
        let path = std::env::var_os("WEBM_INSPECTION_FIXTURE")
            .map(PathBuf::from)
            .expect("set WEBM_INSPECTION_FIXTURE");
        let path = fs::canonicalize(path).expect("resolve WebM fixture");
        let working_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let artifact = Artifact {
            id: "artifact_fixture".into(),
            absolute_path: path,
            relative_path: "artifacts/browser/fixture.webm".into(),
        };
        let host = AgentBrowserHost {
            config: Arc::new(AgentBrowserCliConfig {
                command: PathBuf::new(),
                prefix_args: Vec::new(),
                working_directory: working_directory.clone(),
                artifact_directory: working_directory.join("artifacts/browser"),
                session_name: "fixture".into(),
                profile_directory: working_directory.join("artifacts/browser-profile"),
                executable_path: None,
                cdp_endpoint: None,
                auto_connect: false,
                headed: false,
                allowed_hosts: Vec::new(),
                timeout: Duration::from_secs(60),
            }),
            state: Arc::new(Mutex::new(BrowserState {
                sessions: BTreeMap::new(),
                videos: BTreeMap::from([(artifact.id.clone(), artifact)]),
            })),
        };

        let inspection = host
            .inspect_video("artifact_fixture")
            .await
            .expect("inspect registered WebM fixture");
        assert_eq!(inspection["decodable"], true);
        assert_eq!(inspection["sampled_frames"], 3);
        assert!(inspection["duration_seconds"].as_f64().unwrap() > 0.0);
    }

    #[tokio::test]
    #[ignore = "requires an installed agent-browser and a local Chromium executable"]
    async fn records_agent_browser_smoke_artifacts() {
        let command = std::env::var_os("AGENT_BROWSER_SMOKE_COMMAND")
            .map(PathBuf::from)
            .expect("set AGENT_BROWSER_SMOKE_COMMAND to the native agent-browser executable");
        let executable_path = std::env::var_os("AGENT_BROWSER_SMOKE_EXECUTABLE").map(PathBuf::from);
        let prefix_args = std::env::var("AGENT_BROWSER_SMOKE_BROWSER_ARGS")
            .ok()
            .map(|args| vec!["--args".into(), args])
            .unwrap_or_default();
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let host = AgentBrowserHost::new(AgentBrowserCliConfig {
            command,
            prefix_args,
            working_directory: workspace,
            artifact_directory: PathBuf::from("artifacts/browser"),
            session_name: format!("codex-smoke-{}", opaque_id("session")),
            profile_directory: std::env::var_os("AGENT_BROWSER_SMOKE_PROFILE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("artifacts/browser-profile")),
            executable_path,
            cdp_endpoint: None,
            auto_connect: false,
            headed: false,
            allowed_hosts: vec!["github.com".into()],
            timeout: Duration::from_secs(60),
        })
        .expect("create agent-browser host");

        let attached = host
            .invoke_operation(BrowserOperation::Attach, json!({}))
            .await
            .expect("attach browser");
        let session_id = attached["session_id"].as_str().expect("session id");
        let opened = host
            .invoke_operation(
                BrowserOperation::OpenPage,
                json!({"session_id": session_id, "url": "https://github.com/vercel-labs/agent-browser"}),
            )
            .await
            .expect("open repository");
        let page_id = opened["pages"]
            .as_array()
            .and_then(|pages| pages.iter().find(|page| page["current"] == true))
            .and_then(|page| page["page_id"].as_str())
            .expect("current page id");
        host.invoke_operation(
            BrowserOperation::WaitFor,
            json!({"session_id": session_id, "page_id": page_id, "text": "agent-browser"}),
        )
        .await
        .expect("wait for repository content");

        host.invoke_operation(
            BrowserOperation::VideoStart,
            json!({"session_id": session_id, "page_id": page_id}),
        )
        .await
        .expect("start video");
        let pages = host
            .invoke_operation(
                BrowserOperation::ListPages,
                json!({"session_id": session_id}),
            )
            .await
            .expect("refresh pages after recording starts");
        let page_id = pages["pages"]
            .as_array()
            .and_then(|pages| pages.iter().find(|page| page["current"] == true))
            .and_then(|page| page["page_id"].as_str())
            .expect("current recording page id");
        let recording_started = std::time::Instant::now();
        let actions = async {
            host.invoke_operation(
                BrowserOperation::WaitFor,
                json!({"session_id": session_id, "page_id": page_id, "text": "agent-browser"}),
            )
            .await?;

            let root = smoke_snapshot(&host, session_id, page_id).await?;
            if root.contains("link \"Sign in\"") {
                return Err(HostError::Execution(
                    "the supplied browser profile is not logged in to GitHub".into(),
                ));
            }
            let code = snapshot_ref(&root, &["button \"Code\""])?;
            host.invoke_operation(
                BrowserOperation::Click,
                json!({"session_id": session_id, "page_id": page_id, "target": code}),
            )
            .await?;
            tokio::time::sleep(Duration::from_secs(2)).await;

            host.invoke_operation(
                BrowserOperation::WaitFor,
                json!({"session_id": session_id, "page_id": page_id, "text": "https://github.com/vercel-labs/agent-browser.git"}),
            )
            .await?;
            host.invoke_operation(
                BrowserOperation::Press,
                json!({"session_id": session_id, "page_id": page_id, "key": "Escape"}),
            )
            .await?;
            tokio::time::sleep(Duration::from_secs(1)).await;

            let root = smoke_snapshot(&host, session_id, page_id).await?;
            let docs = snapshot_ref(&root, &["link \"docs\""])?;
            host.invoke_operation(
                BrowserOperation::Click,
                json!({"session_id": session_id, "page_id": page_id, "target": docs}),
            )
            .await?;
            host.invoke_operation(
                BrowserOperation::WaitFor,
                json!({"session_id": session_id, "page_id": page_id, "text": ".gitignore"}),
            )
            .await?;
            tokio::time::sleep(Duration::from_secs(2)).await;

            let docs = smoke_snapshot(&host, session_id, page_id).await?;
            let file = snapshot_ref(&docs, &["link \".gitignore\""])?;
            host.invoke_operation(
                BrowserOperation::Click,
                json!({"session_id": session_id, "page_id": page_id, "target": file}),
            )
            .await?;
            host.invoke_operation(
                BrowserOperation::WaitFor,
                json!({"session_id": session_id, "page_id": page_id, "text": "Edit this file"}),
            )
            .await?;
            tokio::time::sleep(Duration::from_secs(1)).await;

            let file = smoke_snapshot(&host, session_id, page_id).await?;
            let edit = snapshot_ref(
                &file,
                &["button \"Edit this file\"", "link \"Edit this file\""],
            )?;
            host.invoke_operation(
                BrowserOperation::Click,
                json!({"session_id": session_id, "page_id": page_id, "target": edit}),
            )
            .await?;
            tokio::time::sleep(Duration::from_secs(2)).await;

            let editor = smoke_snapshot(&host, session_id, page_id).await?;
            let textbox = snapshot_ref(
                &editor,
                &[
                    "textbox \"Editing file content",
                    "textbox \"Editor content",
                    "textbox",
                ],
            )?;
            host.invoke_operation(
                BrowserOperation::Fill,
                json!({
                    "session_id": session_id,
                    "page_id": page_id,
                    "target": textbox,
                    "text": "# Code Mode browser recording demo\n.next\nout"
                }),
            )
            .await?;
            tokio::time::sleep(Duration::from_secs(2)).await;

            let editor = smoke_snapshot(&host, session_id, page_id).await?;
            let cancel = snapshot_ref(
                &editor,
                &["button \"Cancel changes\"", "button \"Cancel\""],
            )?;
            host.invoke_operation(
                BrowserOperation::Click,
                json!({"session_id": session_id, "page_id": page_id, "target": cancel}),
            )
            .await?;
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok::<(), HostError>(())
        }
        .await;

        let minimum_duration = Duration::from_secs(10);
        if recording_started.elapsed() < minimum_duration {
            tokio::time::sleep(minimum_duration - recording_started.elapsed()).await;
        }
        let screenshot = host
            .invoke_operation(
                BrowserOperation::Screenshot,
                json!({"session_id": session_id, "page_id": page_id}),
            )
            .await;
        let video = host.stop_video(session_id).await;
        let close = host
            .run_cli(Some(&host.config.session_name), vec!["close".into()])
            .await;

        actions.expect("complete recorded GitHub workflow");
        let screenshot = screenshot.expect("capture final screenshot");
        let video = video.expect("stop video");
        close.expect("close smoke browser");
        println!("{}", json!({"screenshot": screenshot, "video": video}));
    }

    #[tokio::test]
    #[ignore = "requires agent-browser and an explicitly enabled existing CDP browser"]
    async fn records_existing_cdp_smoke_artifacts() {
        let command = std::env::var_os("AGENT_BROWSER_SMOKE_COMMAND")
            .map(PathBuf::from)
            .expect("set AGENT_BROWSER_SMOKE_COMMAND to agent-browser");
        let cdp_endpoint = std::env::var("AGENT_BROWSER_SMOKE_CDP")
            .expect("set AGENT_BROWSER_SMOKE_CDP to the existing browser websocket URL");
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let host = AgentBrowserHost::new(AgentBrowserCliConfig {
            command,
            prefix_args: Vec::new(),
            working_directory: workspace.clone(),
            artifact_directory: PathBuf::from("artifacts/browser"),
            session_name: format!("codex-cdp-smoke-{}", opaque_id("session")),
            profile_directory: workspace.join("artifacts/browser-profile"),
            executable_path: None,
            cdp_endpoint: Some(cdp_endpoint),
            auto_connect: false,
            headed: true,
            allowed_hosts: vec!["github.com".into(), "github.dev".into()],
            timeout: Duration::from_secs(60),
        })
        .expect("create existing-CDP agent-browser host");

        let attached = host
            .invoke_operation(BrowserOperation::Attach, json!({}))
            .await
            .expect("attach existing browser");
        let session_id = attached["session_id"].as_str().expect("session id");
        let pages = host
            .invoke_operation(
                BrowserOperation::ListPages,
                json!({"session_id": session_id}),
            )
            .await
            .expect("list existing pages");
        let page_id = pages["pages"]
            .as_array()
            .and_then(|pages| pages.iter().find(|page| page["current"] == true))
            .and_then(|page| page["page_id"].as_str())
            .expect("current existing page id");
        host.invoke_operation(
            BrowserOperation::VideoStart,
            json!({"session_id": session_id, "page_id": page_id}),
        )
        .await
        .expect("start video");
        tokio::time::sleep(Duration::from_secs(10)).await;
        let pages = host
            .invoke_operation(
                BrowserOperation::ListPages,
                json!({"session_id": session_id}),
            )
            .await
            .expect("list recording pages");
        let page_id = pages["pages"]
            .as_array()
            .and_then(|pages| pages.iter().find(|page| page["current"] == true))
            .and_then(|page| page["page_id"].as_str())
            .expect("current recording page id");
        let screenshot = host
            .invoke_operation(
                BrowserOperation::Screenshot,
                json!({"session_id": session_id, "page_id": page_id}),
            )
            .await
            .expect("capture recording page");
        let video = host.stop_video(session_id).await.expect("stop video");
        println!("{}", json!({"screenshot": screenshot, "video": video}));
    }
}
