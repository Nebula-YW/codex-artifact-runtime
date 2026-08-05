import { describe, expect, it } from "vitest";

import { createToolsProxy, type CapabilityRequest } from "./tools-proxy";

describe("createToolsProxy", () => {
  it("maps namespace and operation access to one structured dispatcher", async () => {
    const calls: CapabilityRequest[] = [];
    const tools = createToolsProxy<{
      autoComm: { readSpeed(input: { selector: string }): Promise<{ speedKph: number }> };
    }>(async (request) => {
      calls.push(request);
      return { speedKph: 50 };
    });

    await expect(
      tools.autoComm.readSpeed({ selector: "VehicleStatus.GetSpeed" }),
    ).resolves.toEqual({ speedKph: 50 });
    expect(calls).toEqual([
      {
        namespace: "autoComm",
        operation: "readSpeed",
        input: { selector: "VehicleStatus.GetSpeed" },
      },
    ]);
  });

  it("is not accidentally treated as a promise", () => {
    const tools = createToolsProxy<Record<string, unknown>>(async () => null);
    expect((tools as { then?: unknown }).then).toBeUndefined();
  });
});
