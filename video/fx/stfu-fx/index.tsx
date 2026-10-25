import React from "react";
import { MONO, glow } from "@neon/fx-kit";
import { Heading, Logo, Pill, Stage, useClip } from "./parts.tsx";
import { THEME } from "./theme.ts";

/** Full-frame title: the app icon with breathing bars, the name, a subtitle and a footer. */
export const StfuTitle: React.FC<{ title: string; subtitle: string; footer: string }> = ({ title, subtitle, footer }) => {
  const { sec, enter, out } = useClip(0.4);
  const rise = (t: number, by: number) => ({ opacity: t, transform: `translateY(${(1 - t) * by}px)` });
  const mark = enter(0.1, 0.5);
  return (
    <Stage opacity={out}>
      <div style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", paddingBottom: 60 }}>
        <div style={{ opacity: mark, transform: `scale(${0.85 + 0.15 * mark})`, borderRadius: 60, boxShadow: glow(THEME.wave, 0.35 * mark) }}>
          <Logo size={240} sec={sec} live={enter(0.8, 1)} build={enter(0.1, 1)} />
        </div>
        <div style={{ marginTop: 54, fontSize: 150, fontWeight: 850, letterSpacing: -2, lineHeight: 1, ...rise(enter(0.7), 30) }}>{title}</div>
        {subtitle ? <div style={{ marginTop: 26, fontSize: 46, color: THEME.secondary, ...rise(enter(1.0), 24) }}>{subtitle}</div> : null}
        <div style={{ marginTop: 56, ...rise(enter(1.3), 20) }}>
          <Pill sec={sec} scale={1.7} />
        </div>
      </div>
      {footer ? <div style={{ position: "absolute", left: 0, right: 0, bottom: 96, textAlign: "center", fontFamily: MONO, fontSize: 28, letterSpacing: 1, color: THEME.cyan, opacity: enter(1.6) }}>{footer}</div> : null}
    </Stage>
  );
};

/** Chapter label that slides in at the bottom left over footage. */
export const StfuChapter: React.FC<{ number: number; title: string }> = ({ number, title }) => {
  const { enter, out } = useClip(0.4);
  const inT = enter(0, 0.5);
  return (
    <Stage background={false}>
      <div style={{ position: "absolute", left: 56, bottom: 64, display: "flex", alignItems: "center", gap: 20, padding: "14px 34px 14px 14px", borderRadius: 20, background: "rgba(22,22,26,0.95)", border: `2px solid ${THEME.border}`, boxShadow: "0 16px 50px rgba(0,0,0,0.5)", opacity: Math.min(inT, out), transform: `translateX(${(1 - inT) * -40}px)` }}>
        <div style={{ width: 62, height: 62, borderRadius: 15, background: THEME.cyan, color: "#0b1620", display: "grid", placeItems: "center", fontFamily: MONO, fontSize: 30, fontWeight: 800 }}>{String(number).padStart(2, "0")}</div>
        <div style={{ fontSize: 40, fontWeight: 700 }}>{title}</div>
      </div>
    </Stage>
  );
};

const ICON = { width: 46, height: 46, viewBox: "0 0 24 24", fill: "none", stroke: THEME.wave, strokeWidth: 1.8, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
const icons: Record<string, React.ReactNode> = {
  key: <div style={{ width: 52, height: 46, borderRadius: 9, border: `2.5px solid ${THEME.wave}`, display: "grid", placeItems: "center", fontSize: 21, fontWeight: 800, color: THEME.wave, fontFamily: MONO }}>fn</div>,
  mic: <svg {...ICON}><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0M12 18v3" /></svg>,
  text: <svg {...ICON}><path d="M4 7h16M4 12h12M4 17h8" /></svg>,
  clean: <svg {...ICON}><path d="M12 3l1.8 4.7L18.5 9.5l-4.7 1.8L12 16l-1.8-4.7L5.5 9.5l4.7-1.8z" /><path d="M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z" /></svg>,
  paste: <svg {...ICON}><rect x="5" y="4" width="14" height="17" rx="2" /><path d="M9 4V3h6v1M9 11h6M9 15h4" /></svg>,
};

const NODES = [
  { icon: "key", title: "Hotkey", lines: ["Fn on macOS", "Ctrl+Win on Windows"], endpoint: "hold to record" },
  { icon: "mic", title: "Microphone", lines: ["Live levels", "to the pill"], endpoint: "WAV on release" },
  { icon: "text", title: "Speech to text", lines: ["Groq", "whisper-large-v3-turbo"], endpoint: "POST /audio/transcriptions" },
  { icon: "clean", title: "LLM cleanup", lines: ["Groq", "qwen/qwen3.8-27b"], endpoint: "POST /chat/completions" },
  { icon: "paste", title: "Paste", lines: ["Clipboard, then", "Cmd or Ctrl+V"], endpoint: "old clipboard restored" },
];
/** Which nodes each step lights up. */
const ACTIVE: Record<number, number[]> = { 1: [0, 1], 2: [2], 3: [3], 4: [4], 5: [2, 4] };
const XS = [190, 575, 960, 1345, 1730];
const Y = 470;
const W = 316;

/** The pipeline from hotkey to paste, built up one step per narrated line, with the endpoints. */
export const StfuDiagram: React.FC<{ step: number }> = ({ step }) => {
  const { sec, enter, out } = useClip(0.35);
  const active = ACTIVE[step] ?? [];
  const reached = Math.max(...active);
  const pulse = 0.5 + 0.5 * Math.sin(sec * 4);
  const show = (i: number) => (i <= reached ? 1 : 0.28);
  const now = enter(0.2, 0.7);
  return (
    <Stage opacity={out}>
      <Heading kicker="How it works" text="Hold, talk, release, paste" show={enter(0, 0.5)} />
      {/* Arrows, with a dot travelling into the step being described. */}
      {XS.slice(0, -1).map((x, i) => {
        const from = x + W / 2 + 8;
        const to = XS[i + 1]! - W / 2 - 8;
        const lit = i + 1 <= reached && !(step === 5 && i === 2);
        const travelling = active.includes(i + 1) && step !== 5;
        const t = (sec * 0.8) % 1;
        return (
          <React.Fragment key={i}>
            <div style={{ position: "absolute", left: from, top: Y - 2, width: to - from, height: 4, borderRadius: 2, background: lit ? THEME.cyan : THEME.border, opacity: step === 5 && i === 2 ? 0.35 : 1 }} />
            <div style={{ position: "absolute", left: to - 12, top: Y - 10, width: 0, height: 0, borderTop: "10px solid transparent", borderBottom: "10px solid transparent", borderLeft: `14px solid ${lit ? THEME.cyan : THEME.border}` }} />
            {travelling ? <div style={{ position: "absolute", left: from + (to - from - 16) * t, top: Y - 8, width: 16, height: 16, borderRadius: 8, background: THEME.wave, boxShadow: glow(THEME.wave, 0.8) }} /> : null}
          </React.Fragment>
        );
      })}
      {NODES.map((n, i) => {
        const on = active.includes(i);
        return (
          <div key={i} style={{ position: "absolute", left: XS[i]! - W / 2, top: Y - 120, width: W, height: 240, boxSizing: "border-box", padding: "26px 24px", borderRadius: 24, background: THEME.panel, border: `2px solid ${on ? THEME.cyan : THEME.border}`, boxShadow: on ? `${glow(THEME.cyan, 0.25 + 0.25 * pulse)}, 0 18px 50px rgba(0,0,0,0.45)` : "0 18px 50px rgba(0,0,0,0.45)", opacity: show(i), transform: on ? `scale(${1 + 0.03 * now})` : undefined }}>
            {icons[n.icon]}
            <div style={{ fontSize: 33, fontWeight: 750, marginTop: 18, whiteSpace: "nowrap" }}>{n.title}</div>
            {n.lines.map((l) => <div key={l} style={{ fontSize: 21, color: THEME.muted, marginTop: 6, whiteSpace: "nowrap" }}>{l}</div>)}
            <div style={{ position: "absolute", left: 0, right: 0, top: 262, textAlign: "center", fontFamily: MONO, fontSize: 21, color: on ? THEME.cyan : THEME.muted, whiteSpace: "nowrap" }}>{n.endpoint}</div>
          </div>
        );
      })}
      {step === 1 ? (
        <div style={{ position: "absolute", left: 0, right: 0, top: 790, display: "flex", justifyContent: "center", opacity: enter(0.6) }}>
          <Pill sec={sec} scale={2.2} />
        </div>
      ) : null}
      {step >= 3 ? (
        <div style={{ position: "absolute", left: 120, right: 120, top: 770, display: "flex", gap: 22, justifyContent: "center", opacity: step === 3 ? enter(0.5) : 0.55 }}>
          {["Fillers removed", "Corrections applied", "Punctuation fixed", "Lists become bullets", "Never answers you"].map((r, i) => (
            <div key={r} style={{ padding: "16px 26px", borderRadius: 16, background: THEME.base, border: `2px solid ${THEME.border}`, fontSize: 27, fontWeight: 600, opacity: step === 3 ? enter(0.6 + i * 0.25) : 1, color: i === 4 ? THEME.yellow : THEME.text }}>{r}</div>
          ))}
        </div>
      ) : null}
      {step === 5 ? (
        <>
          <svg width="1920" height="1080" style={{ position: "absolute", left: 0, top: 0 }}>
            <path d={`M ${XS[2]} ${Y - 130} C ${XS[2]! + 120} ${Y - 290}, ${XS[4]! - 120} ${Y - 290}, ${XS[4]} ${Y - 130}`} fill="none" stroke={THEME.yellow} strokeWidth={5} strokeDasharray="16 12" strokeDashoffset={-sec * 40} pathLength={1000} opacity={enter(0.3)} />
          </svg>
          <div style={{ position: "absolute", left: XS[3]! - 330, width: 660, top: Y - 290, textAlign: "center", fontSize: 28, fontWeight: 700, color: THEME.yellow, opacity: enter(0.5) }}>Cleanup fails or times out: paste the raw transcript</div>
          <div style={{ position: "absolute", left: 0, right: 0, top: 960, textAlign: "center", fontSize: 28, color: THEME.muted, opacity: enter(1.2) }}>
            Rust core on <span style={{ color: THEME.text }}>Tauri 2</span> · two small webviews: the <span style={{ color: THEME.text }}>pill</span> and <span style={{ color: THEME.text }}>Settings</span>
          </div>
        </>
      ) : null}
    </Stage>
  );
};

const INSTALL = [
  { label: "macOS or Linux", prompt: "$", command: "curl -fsSL https://raw.githubusercontent.com/joe-lloyd/stfu/main/scripts/install.sh | sh" },
  { label: "Windows, in PowerShell", prompt: "PS>", command: "irm https://raw.githubusercontent.com/joe-lloyd/stfu/main/scripts/install.ps1 | iex" },
];

/** The two one-line installers, typed out in terminal panels. */
export const StfuInstall: React.FC = () => {
  const { sec, enter, out } = useClip(0.35);
  return (
    <Stage opacity={out}>
      <Heading kicker="Install" text="One line on every platform" show={enter(0, 0.5)} />
      {INSTALL.map((c, i) => {
        const start = 0.6 + i * 3.2;
        const typed = Math.max(0, Math.min(c.command.length, Math.floor((sec - start) * 40)));
        const cursorOn = Math.floor(sec * 2) % 2 === 0;
        return (
          <div key={c.label} style={{ position: "absolute", left: 120, right: 120, top: 300 + i * 250, opacity: enter(0.3 + i * 0.4), transform: `translateY(${(1 - enter(0.3 + i * 0.4)) * 24}px)` }}>
            <div style={{ fontSize: 28, fontWeight: 700, color: THEME.secondary, marginBottom: 14 }}>{c.label}</div>
            <div style={{ borderRadius: 18, background: "#0b0b0e", border: `2px solid ${THEME.border}`, padding: "28px 32px", fontFamily: MONO, fontSize: 29, whiteSpace: "nowrap", boxShadow: "0 18px 50px rgba(0,0,0,0.45)" }}>
              <span style={{ color: THEME.green }}>{c.prompt} </span>
              <span>{c.command.slice(0, typed)}</span>
              <span style={{ display: "inline-block", width: 16, height: 32, verticalAlign: "-5px", background: THEME.cyan, opacity: typed < c.command.length || cursorOn ? 0.9 : 0 }} />
            </div>
          </div>
        );
      })}
      <div style={{ position: "absolute", left: 120, right: 120, top: 860, fontSize: 28, color: THEME.muted, opacity: enter(7.5) }}>
        Both clear the download quarantine, because the builds aren't signed by Apple or Microsoft. Pin a version with <span style={{ fontFamily: MONO, color: THEME.text }}>STFU_VERSION=v0.9.2</span>.
      </div>
      <div style={{ position: "absolute", left: 120, top: 960, fontFamily: MONO, fontSize: 26, color: THEME.cyan, opacity: enter(8.5) }}>github.com/joe-lloyd/stfu/releases</div>
    </Stage>
  );
};

/**
 * YouTube thumbnail laid over a frame of the video: a dark panel on the left with the icon, the
 * name and the kicker, and the footage showing through on the right. Static, so every frame matches.
 */
export const StfuThumbnail: React.FC<{ title: string; kicker: string }> = ({ title, kicker }) => (
  <Stage background={false}>
    <div style={{ position: "absolute", inset: 0, background: `linear-gradient(90deg, ${THEME.deepest} 0%, ${THEME.deepest} 44%, rgba(14,14,18,0.85) 58%, rgba(14,14,18,0.2) 82%, rgba(14,14,18,0) 100%)` }} />
    <div style={{ position: "absolute", inset: 0, background: "radial-gradient(900px 700px at 20% 45%, rgba(125,211,252,0.16), transparent 70%)" }} />
    <div style={{ position: "absolute", left: 130, top: 120 }}>
      <Logo size={190} sec={0.4} />
    </div>
    <div style={{ position: "absolute", left: 120, top: 330, fontSize: 330, fontWeight: 900, letterSpacing: -10, lineHeight: 1, color: THEME.text, textShadow: "0 8px 40px rgba(0,0,0,0.6)" }}>{title}</div>
    <div style={{ position: "absolute", left: 132, top: 700, width: 1000, fontSize: 70, fontWeight: 800, lineHeight: 1.15, color: THEME.cyan }}>{kicker}</div>
    <div style={{ position: "absolute", left: 132, top: 900 }}>
      <Pill sec={0.9} scale={2.6} />
    </div>
  </Stage>
);
