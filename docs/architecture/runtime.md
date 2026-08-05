# Runtime architecture

Updated: 2026-08-05

## Boundary

Codex Artifact Runtime does not implement Code Mode. It uses Codex App Server's native JavaScript Code Mode and injects project-authorized tools at `thread/start`.

```text
Codex App Server client
  -> initialize(capabilities.experimentalApi = true)
  -> thread/start(dynamicTools = request operations)
  -> Codex native Code Mode calls tools.<namespace>.<operation>(input)
  -> item/tool/call
  -> Capability Host
  -> CLI / HTTP / native adapter
  -> item/tool/call response
```

Ordinary users keep the official Codex installation. The `codex-artifact` companion runs a loopback WebSocket Gateway in front of the official App Server stdio transport:

```text
official codex TUI --remote ws://127.0.0.1:<port>
  -> codex-artifact Gateway
       -> official codex app-server --stdio
```

The Gateway preserves ordinary protocol traffic, adds `experimentalApi` during initialization, adds the authorized Dynamic Tool snapshot during `thread/start`, and handles `item/tool/call` locally. It never rewrites model output or replaces the native Code Mode host.

This is a companion installation boundary, not a Codex fork. A user keeps the official `codex` executable and adds only `codex-artifact`. The companion discovers `codex` from `PATH` by default (or accepts `--codex-bin`), launches its official App Server and TUI processes, and terminates them with the Gateway. Prebuilt companion binaries can therefore be distributed independently on Windows, Linux, and macOS.

The TSX Application does not call Codex. It consumes another projection of the same catalog:

```text
Capability Catalog
  +-> Codex DynamicToolSpec[]
  +-> Application tools.d.ts
  +-> browser tools proxy
  +-> Host input/output validation
```

## Canonical contract

The catalog, not Codex or the TSX source, owns operation identity and JSON Schema. A capability is identified by `<namespace>.<operation>`.

Operations have two kinds:

- `request`: one input and one terminal result; eligible for Codex `dynamicTools` and TSX `Promise<Result>` calls.
- `stream`: a subscription lifecycle; available to TSX as an async stream in a later Host transport, but intentionally excluded from Codex `dynamicTools`.

## Invocation

The Host owns bindings. Tool arguments cannot select an executable, endpoint, credential, or working directory. CLI bindings use a direct child process with an argument vector and JSON stdin/stdout. They never invoke PowerShell, `cmd.exe`, or `/bin/sh`.

Dynamic Tool registration is not authorization. The `CapabilityHost` handler boundary is where a product adds identity, project grants, approval, deadlines, audit, and credential resolution before executing a binding.

## Application isolation

Application TSX receives a generated `tools` proxy. The iframe sends structured `{ namespace, operation, input }` messages to its parent Host. It does not receive credentials, native paths, CLI bindings, or arbitrary network access.

The dynamic TSX compiler and sandbox shell will be extracted as a self-contained component after the shared capability contract is stable. The new repository remains self-contained and does not use path dependencies on another workspace.
