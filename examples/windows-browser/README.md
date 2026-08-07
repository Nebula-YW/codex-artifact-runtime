# Windows browser through native Code Mode

This example keeps Codex's native JavaScript Code Mode as the only agent runtime. Codex sees structured `tools.webBrowser.*`, `tools.fs.*`, and `tools.approval.*` methods generated from one capability catalog. The Capability Host invokes `agent-browser` as a local executable with an argument array; it does not expose `agent-browser` chat, MCP, arbitrary JavaScript, or a second tool transport.

Known Windows and cross-machine failure modes are tracked in the [browser troubleshooting record](../../docs/troubleshooting/windows-browser-and-cross-machine.md). That record is intentionally updated as additional machines complete the smoke test.

`webBrowser` is intentionally not named `browser`, because Codex reserves that namespace on some surfaces.

## 1. Install the browser driver

On Windows with Node.js/npm on `PATH`:

```powershell
npm.cmd install -g agent-browser
agent-browser.cmd install
```

Video recording also requires `ffmpeg` on `PATH`; screenshots and ordinary browser actions do not. The Windows Host resolves the npm shim to `agent-browser`'s packaged native `.exe` before launch, so capability calls never run through `cmd.exe` or PowerShell.

The binding uses a dedicated persistent profile at `artifacts/browser-profile`. The browser window is headed by default. There is no extension authorization prompt: the Host starts or reuses this profile itself.

For signed-in pages, log in once in that dedicated profile and later runs reuse its cookies and local storage. Ordinary username/password forms can instead be driven with `snapshot`, `fill`, `press`, and `click` after starting the companion with `--allow-side-effects`. MFA, CAPTCHA, passkeys, WebAuthn, and identity-provider consent may still require user interaction in the headed window. Stored browser secrets are never returned to Code Mode.

## 2. Configure the Host boundary

Edit [`bindings.windows.json`](bindings.windows.json) before starting:

- Add only the exact site hosts needed by the task to `policy.allowedHosts`.
- Keep `fileRoots.workspace`, `agentBrowser.workingDirectory`, `agentBrowser.profileDirectory`, and the artifact directory scoped to the intended workspace.
- `allowLocalWrites` permits screenshots, recordings, downloads, and `fs.writeText` under that workspace.
- Leave `allowLowRiskBrowserActions` false unless account-changing low-risk clicks are intentionally in scope.

Tool visibility is only discovery. Every invocation still passes through the Capability Host policy guard. Browser-internal URLs, credential-bearing URLs, local files, non-allowlisted hosts, arbitrary selectors, and arbitrary browser scripts are rejected.

The Host also strips inherited `AGENT_BROWSER_*` environment overrides before every invocation. Browser launch options must come from the trusted binding's `agentBrowser.args` array.

## 3. Generate both projections from one catalog

```powershell
cargo run -p capability-schema-cli -- examples/windows-browser/capabilities.json validate
cargo run -p capability-schema-cli -- examples/windows-browser/capabilities.json typescript --output examples/windows-browser/tools.d.ts
```

The same catalog supplies Dynamic Tools at `thread/start` and the TSX `tools` declarations.

## 4. Start native Code Mode

From the repository root:

```powershell
codex-artifact run --catalog examples/windows-browser/capabilities.json --bindings examples/windows-browser/bindings.windows.json -- -C (Get-Location).Path
```

An example recording flow is:

```js
const session = await tools.webBrowser.attach({});
let opened = await tools.webBrowser.openPage({
  session_id: session.session_id,
  url: "https://github.com/Nebula-YW/codex-artifact-runtime",
});
let page = opened.pages.find(item => item.current);

await tools.webBrowser.videoStart({
  session_id: session.session_id,
  page_id: page.page_id,
});

// Recording creates a fresh browser context, so refresh the opaque page handle.
({ pages: opened } = await tools.webBrowser.listPages({
  session_id: session.session_id,
}));
page = opened.find(item => item.current);

const snapshot = await tools.webBrowser.snapshot({
  session_id: session.session_id,
  page_id: page.page_id,
});
await tools.webBrowser.scroll({
  session_id: session.session_id,
  page_id: page.page_id,
  delta_y: 700,
});
const shot = await tools.webBrowser.screenshot({
  session_id: session.session_id,
  page_id: page.page_id,
});
const video = await tools.webBrowser.videoStop({
  session_id: session.session_id,
});
const inspection = await tools.webBrowser.videoInspect({
  artifact_id: video.artifact_id,
});
```

Video stop is deliberately session-scoped: recording replaces the browser context, so it must not depend on the pre-recording page handle. The Host checks the WebM header before returning the artifact and removes an incomplete output. `videoInspect` accepts only a Host-registered opaque artifact ID; it uses FFprobe/FFmpeg inside the same policy seam to report decode status, duration, and sampled-frame variation without granting arbitrary file access.

Screenshots, WebM recordings, and downloads are returned as logical workspace paths under `artifacts/browser/`. Accessibility snapshots contain compact element refs such as `e12`; the Host converts only validated refs to `agent-browser` arguments.

## Safety and approval behavior

- Reads and navigation on allowlisted sites run automatically.
- Local artifact writes require `allowLocalWrites`.
- A click labelled `intent: "read"` may expand or paginate visible content.
- Low-risk account changes require `allowLowRiskBrowserActions`.
- Form filling, key presses, high-risk clicks, and page closing require a deliberate restart with `--allow-side-effects`.
- `approval.request` only reports the approval requirement; it never grants permission itself.
