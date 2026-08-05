import { createToolsProxy, type CapabilityRequest } from "./tools-proxy";

export const APPLICATION_CHANNEL = "codex-artifact-runtime.application.v1";

type Pending = {
  resolve(value: unknown): void;
  reject(error: Error): void;
};

export function createIframeTools<T extends object>(target: Window = window.parent): T {
  let nextId = 1;
  const pending = new Map<string, Pending>();

  window.addEventListener("message", (event: MessageEvent<unknown>) => {
    if (event.source !== target || !event.data || typeof event.data !== "object") return;
    const message = event.data as Record<string, unknown>;
    if (message.channel !== APPLICATION_CHANNEL || typeof message.id !== "string") return;
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    if (message.type === "resolved") {
      request.resolve(message.result);
    } else if (message.type === "rejected") {
      request.reject(new Error(typeof message.error === "string" ? message.error : "Capability failed"));
    }
  });

  const dispatch = (request: CapabilityRequest) =>
    new Promise<unknown>((resolve, reject) => {
      const id = `capability-${nextId++}`;
      pending.set(id, { resolve, reject });
      target.postMessage(
        {
          channel: APPLICATION_CHANNEL,
          type: "invoke",
          id,
          ...request,
        },
        "*",
      );
    });

  return createToolsProxy<T>(dispatch);
}
