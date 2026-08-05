# Repository guidance

- Keep Codex native JavaScript Code Mode as the only agent code runtime.
- Do not add an alternate Code Mode runtime or a second tool transport.
- Derive Codex Dynamic Tools and TSX `tools` declarations from the same capability catalog.
- Invoke local programs with an executable and argument array; never construct shell command strings.
- Treat tool visibility as discovery, not authorization. Every invocation must pass through the Host policy seam.
- Keep request/response operations separate from streaming subscriptions.
- Preserve Windows, macOS, and Linux portability.
- Run Rust and Application Runtime tests before committing.
