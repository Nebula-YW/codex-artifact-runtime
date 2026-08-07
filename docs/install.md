# Install the companion

The distributed `codex-artifact` executable is a companion to the official Codex CLI. It does not contain, replace, or patch Codex.

## Prerequisite

Install the official Codex CLI first and verify that it is available on `PATH`:

```text
codex --version
```

The initial companion release is tested with `codex-cli 0.146.0`. Codex Dynamic Tools are still an experimental App Server surface, so a future incompatible protocol change may require a companion update.

Then download the archive for your platform from the repository's GitHub Releases page.

| Platform | Release asset suffix |
| --- | --- |
| Linux x64 | `x86_64-unknown-linux-musl.tar.gz` |
| Linux ARM64 | `aarch64-unknown-linux-musl.tar.gz` |
| macOS Intel | `x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin.tar.gz` |
| Windows x64 | `x86_64-pc-windows-msvc.zip` |

Linux archives are statically linked against musl so they do not require the build machine's glibc version.

## macOS and Linux

Extract the downloaded archive, then place the executable in a directory on `PATH`:

```bash
tar -xzf codex-artifact-v0.1.0-<target>.tar.gz
install -m 0755 codex-artifact-v0.1.0-<target>/codex-artifact "$HOME/.local/bin/codex-artifact"
codex-artifact --version
```

On macOS, a downloaded unsigned binary may require an explicit approval in **System Settings → Privacy & Security**. Code signing and notarization are separate distribution hardening work; the initial workflow does not claim either.

## Windows

Extract the ZIP file and place `codex-artifact.exe` in a directory listed in the user `PATH`. Open a new terminal and verify:

```powershell
codex-artifact --version
```

No PowerShell or `cmd.exe` process is used when the companion invokes a capability binding. Windows installation only uses the shell to unpack or move the executable.

For browser automation, install `agent-browser` and its managed Chromium build. Install `ffmpeg` on `PATH` if video recording is needed. The Windows example resolves the npm launcher to the packaged native executable and launches it with an argument array; the Host never constructs a PowerShell or `cmd.exe` command string. Follow the [Windows browser walkthrough](../examples/windows-browser/README.md) after installing the companion.

## Verify the download

Every Release includes `SHA256SUMS`. Compare the hash of the archive before extracting it.

Linux:

```bash
sha256sum codex-artifact-v0.1.0-<target>.tar.gz
```

macOS:

```bash
shasum -a 256 codex-artifact-v0.1.0-<target>.tar.gz
```

Windows:

```powershell
Get-FileHash .\codex-artifact-v0.1.0-x86_64-pc-windows-msvc.zip -Algorithm SHA256
```

## Start the official TUI

The catalog and binding files normally come from an Application package or project:

```bash
codex-artifact run \
  --catalog path/to/capabilities.json \
  --bindings path/to/bindings.json \
  -- -C path/to/workspace
```

The companion finds the official `codex` executable through `PATH`. Use `--codex-bin <path>` only when it is installed elsewhere.
Add `--require-new-thread` when the run must receive project Dynamic Tools at `thread/start`; a resume attempt then fails with `ENTRY_NOT_CLOSED` and must be restarted as a fresh thread.

## Release process

Maintainers publish a version by updating the workspace package version and pushing the matching semantic tag, for example `v0.1.0`. The Release workflow rejects a tag that does not exactly match the Cargo package version, builds each platform independently, and publishes the archives with a combined SHA-256 checksum file.
