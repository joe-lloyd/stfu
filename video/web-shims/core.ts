// Stands in for @tauri-apps/api/core in a plain browser: every invoke is answered by the mock
// backend on the top window, so the pill, settings and the stage share one state.
import { backend } from "./mock.ts";

export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return (await backend().invoke(command, args)) as T;
}
