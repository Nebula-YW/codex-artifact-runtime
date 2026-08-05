import { useState } from "react";

declare const tools: {
  autoComm: {
    inspectNetwork(input: { model: string; localNode: string }): Promise<{
      nodes: string[];
      links: string[];
      readiness: "ready" | "partial" | "blocked";
    }>;
    injectFault(input: {
      target: string;
      action: "drop" | "delay" | "freeze" | "rewrite";
      value?: unknown;
    }): Promise<{ faultId: string; status: "active" | "rejected" }>;
  };
};

export default function VehicleNetworkTest() {
  const [status, setStatus] = useState("Inspect the network before injecting a fault.");

  async function inspect() {
    const result = await tools.autoComm.inspectNetwork({
      model: "bench.yaml",
      localNode: "bench-client",
    });
    setStatus(`${result.readiness}: ${result.nodes.length} nodes, ${result.links.length} links`);
  }

  async function inject() {
    const result = await tools.autoComm.injectFault({
      target: "can.signal.VehicleSpeed",
      action: "freeze",
      value: 0,
    });
    setStatus(`Fault ${result.faultId}: ${result.status}`);
  }

  return (
    <main>
      <h1>Vehicle network fault injection</h1>
      <p>{status}</p>
      <button onClick={inspect}>Inspect network</button>
      <button onClick={inject}>Freeze vehicle speed</button>
    </main>
  );
}
