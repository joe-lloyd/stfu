// neon-cli against the STFU tour's own Neon instance, so it never collides with other projects
// using the shared one in ~/.neon-video.
//
//   node video/tools/neon.ts serve | stop | <any neon-cli arguments>
import { execFileSync } from "node:child_process";
import { mkdirSync, symlinkSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

// NEON_HOME picks another instance, e.g. one for uploads while this one renders.
/** The tour's Neon project. */
export const PROJECT = join(homedir(), "Videos", "tours", "stfu.neon");

export const NEON_HOME = process.env.NEON_HOME ?? join(homedir(), ".neon-video-tour-stfu");
const env = { ...process.env, NEON_HOME, PATH: `${homedir()}/.bun/bin:${homedir()}/.local/bin:${process.env.PATH}` };

function prepare() {
  mkdirSync(NEON_HOME, { recursive: true });
  // Reuse the ffmpeg and friends the shared install already downloaded.
  if (!existsSync(join(NEON_HOME, "tools"))) symlinkSync(join(homedir(), ".neon-video", "tools"), join(NEON_HOME, "tools"));
}

export function neon<T = unknown>(...args: string[]): T {
  prepare();
  let raw: string;
  try {
    raw = execFileSync("neon-cli", [...args, "--json"], { env, maxBuffer: 64 * 1024 * 1024 }).toString();
  } catch (error) {
    const out = error instanceof Error && "stdout" in error ? String(error.stdout) : String(error);
    throw new Error(`neon-cli ${args.join(" ")} failed: ${out.slice(0, 2000)}`);
  }
  const parsed: { ok: boolean; data?: T; error?: { message: string } } = JSON.parse(raw);
  if (!parsed.ok) throw new Error(`neon-cli ${args.join(" ")}: ${parsed.error?.message}`);
  return parsed.data as T;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  prepare();
  execFileSync("neon-cli", process.argv.slice(2), { env, stdio: "inherit" });
}
