# Codex native Dynamic Tools

Updated: 2026-08-05

Codex CLI 0.146 generates the following experimental App Server contract when invoked with `codex app-server generate-ts --experimental`:

```text
initialize.capabilities.experimentalApi = true
thread/start.dynamicTools: DynamicToolSpec[]
item/tool/call: DynamicToolCallParams
DynamicToolCallResponse { contentItems, success }
```

A namespaced request tool is projected as:

```json
{
  "type": "namespace",
  "name": "autoComm",
  "description": "Vehicle communication operations",
  "tools": [
    {
      "type": "function",
      "name": "call",
      "description": "Call a resolved service operation",
      "inputSchema": { "type": "object" },
      "deferLoading": true
    }
  ]
}
```

When invoked, the App Server client receives:

```json
{
  "method": "item/tool/call",
  "id": 42,
  "params": {
    "threadId": "thread-id",
    "turnId": "turn-id",
    "callId": "call-id",
    "namespace": "autoComm",
    "tool": "call",
    "arguments": {}
  }
}
```

The client returns text, image, or audio content items plus a success flag. This project initially returns one JSON-encoded `inputText` item so results remain lossless and protocol-compatible.

Important constraints:

- Dynamic Tools are experimental and the Codex version must be pinned and contract-tested.
- Thread creation installs the tool snapshot; invocation-time policy must handle later authorization revocation.
- The request/response callback is not a subscription transport.
- `--code-mode-host` selects a Code Mode execution backend and is not needed when using Codex's native JavaScript runtime.
- OpenAPI or CLI descriptions are normalized before projection; Codex configuration is not the capability catalog.

Official reference: <https://learn.chatgpt.com/docs/app-server#dynamic-tool-calls-experimental>
