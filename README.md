# Codex Artifact Runtime

Codex Artifact Runtime connects Codex native Code Mode and sandboxed TSX Applications to the same dynamically registered capabilities.

```text
                         +-> Codex App Server dynamicTools -> native Code Mode
Capability Catalog -----+
                         +-> generated TypeScript -> TSX tools proxy
                                      |
                                      v
                              Capability Host
                              +-> CLI (no shell)
                              +-> HTTP
                              +-> native Rust
```

Codex owns JavaScript Code Mode. This project does not add another code execution runtime or tool transport; it owns capability contracts, invocation, authorization seams, and the TSX Application bridge.

## Current vertical slice

- validate a versioned, namespaced capability catalog;
- project request operations to Codex App Server `dynamicTools`;
- handle `item/tool/call` with the generated Codex 0.146 protocol shape;
- execute host-owned CLI bindings through `command + args[]` without a shell;
- generate the matching `tools.<namespace>.<operation>()` TypeScript declarations;
- provide a browser-side proxy for TSX Applications;
- keep streaming operations in the shared catalog without pretending that Codex Dynamic Tools are streaming calls.

Inspect the included vehicle-network example:

```bash
cargo run -p capability-schema-cli -- \
  examples/vehicle-network/capabilities.json codex

cargo run -p capability-schema-cli -- \
  examples/vehicle-network/capabilities.json typescript
```

Verify everything:

```bash
cargo test --workspace
pnpm --dir apps/application-runtime test
pnpm --dir apps/application-runtime typecheck
pnpm --dir apps/application-runtime build
```

## Use the official Codex TUI

The companion command starts the installed official `codex app-server`, exposes a loopback-only protocol Gateway, and then connects the installed official TUI. It does not bundle or patch Codex:

Users install two independent executables: the official Codex CLI and this small companion. They do not need a custom Codex build. During local development, install the companion from this checkout:

```bash
cargo install --path crates/codex-artifact-cli --locked
codex-artifact --help
```

Once release binaries are published, ordinary users will install the matching macOS, Linux, or Windows binary and will not need Rust or this source repository. The capability catalog and bindings can be distributed with an Application package or selected from a project directory.

Run the installed companion while continuing to use the official TUI:

```bash
codex-artifact run \
  --catalog examples/vehicle-network/capabilities.json \
  --bindings examples/vehicle-network/bindings.smoke.json \
  -- -C "$PWD"
```

The equivalent development command is:

```bash
cargo run -p codex-artifact-cli --bin codex-artifact -- \
  run \
  --catalog examples/vehicle-network/capabilities.json \
  --bindings examples/vehicle-network/bindings.smoke.json \
  -- -C "$PWD"
```

`bindings.smoke.json` intentionally contains no executable operations and is only useful for protocol verification. A real installation supplies Host-owned CLI, HTTP, or native bindings for request operations. Side-effect operations are denied unless the user starts the companion with `--allow-side-effects` after reviewing those bindings.

For a headless or manually connected TUI, add `--no-tui`; the command prints the exact `codex --remote ws://127.0.0.1:<port>` invocation.

See [architecture](docs/architecture/runtime.md) and [Codex protocol research](docs/research/codex-dynamic-tools.md).
