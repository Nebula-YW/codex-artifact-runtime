export type CapabilityRequest = {
  namespace: string;
  operation: string;
  input: unknown;
};

export type CapabilityDispatcher = (request: CapabilityRequest) => Promise<unknown>;

export function createToolsProxy<T extends object>(dispatch: CapabilityDispatcher): T {
  const namespaces = new Map<string, object>();
  return new Proxy(
    {},
    {
      get(_target, namespace) {
        if (typeof namespace !== "string") return undefined;
        if (namespace === "then") return undefined;
        let operations = namespaces.get(namespace);
        if (!operations) {
          operations = new Proxy(
            {},
            {
              get(_operationTarget, operation) {
                if (typeof operation !== "string") return undefined;
                if (operation === "then") return undefined;
                return (input: unknown) => dispatch({ namespace, operation, input });
              },
            },
          );
          namespaces.set(namespace, operations);
        }
        return operations;
      },
    },
  ) as T;
}
