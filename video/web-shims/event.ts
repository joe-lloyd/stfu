// Stands in for @tauri-apps/api/event in a plain browser. The pill and settings pages run in
// iframes of the stage page, so the listeners live on the top window, where the stage and the
// recording script can emit to every page at once.
type Handler = (event: { event: string; id: number; payload: unknown }) => void;
type Bus = { handlers: Map<string, Set<Handler>>; emit: (name: string, payload?: unknown) => void };

function bus(): Bus {
  const top = window.top as unknown as { __stfuBus?: Bus };
  if (!top.__stfuBus) {
    const handlers = new Map<string, Set<Handler>>();
    top.__stfuBus = {
      handlers,
      emit(name, payload) {
        for (const handler of handlers.get(name) ?? []) {
          // A page that reloaded leaves a dead handler behind; drop it.
          try {
            handler({ event: name, id: 0, payload });
          } catch {
            handlers.get(name)?.delete(handler);
          }
        }
      },
    };
  }
  return top.__stfuBus;
}

export type UnlistenFn = () => void;

export async function listen<T>(name: string, handler: (event: { event: string; id: number; payload: T }) => void): Promise<UnlistenFn> {
  const b = bus();
  const set = b.handlers.get(name) ?? new Set<Handler>();
  b.handlers.set(name, set);
  set.add(handler as Handler);
  return () => set.delete(handler as Handler);
}

export async function emit(name: string, payload?: unknown): Promise<void> {
  bus().emit(name, payload);
}
