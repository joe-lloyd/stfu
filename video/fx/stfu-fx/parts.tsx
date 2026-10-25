import React from "react";
import { AbsoluteFill, Easing, interpolate, useCurrentFrame, useVideoConfig } from "@neon/motion";
import { SANS, useUiScale } from "@neon/fx-kit";
import { THEME } from "./theme.ts";

const CLAMP = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;

/** Frame-derived timing for one clip. `enter` eases 0 → 1 after `delay` seconds; `out` fades at the clip's end. */
export function useClip(outSeconds = 0.35) {
  const frame = useCurrentFrame();
  const { fps, durationInFrames } = useVideoConfig();
  const enter = (delay: number, length = 0.6) =>
    interpolate(frame, [fps * delay, fps * (delay + length)], [0, 1], { ...CLAMP, easing: Easing.out(Easing.cubic) });
  const out = interpolate(frame, [durationInFrames - fps * outSeconds, durationInFrames], [1, 0], CLAMP);
  return { frame, fps, sec: frame / fps, enter, out };
}

/** The faint waveform along the bottom of every card, drifting slowly. */
function WaveFloor({ sec }: { sec: number }) {
  return (
    <div style={{ position: "absolute", left: 0, right: 0, bottom: -40, height: 300, display: "flex", gap: 14, alignItems: "flex-end", justifyContent: "center", opacity: 0.08 }}>
      {Array.from({ length: 64 }, (_, i) => (
        <div key={i} style={{ width: 16, borderRadius: 8, background: i % 9 === 0 ? "#f5276c" : THEME.wave, height: 30 + Math.abs(Math.sin(i * 0.31 + sec * 0.6) * Math.cos(i * 0.12 - sec * 0.3)) * 260 }} />
      ))}
    </div>
  );
}

/** A 1920x1080 design canvas scaled to the output and centred, so children are written in 1080p pixels. */
export function Stage({ children, background = true, opacity = 1 }: { children: React.ReactNode; background?: boolean; opacity?: number }) {
  const s = useUiScale();
  const { width, height, fps } = useVideoConfig();
  const sec = useCurrentFrame() / fps;
  return (
    <AbsoluteFill style={{ opacity, fontFamily: SANS, background: background ? `radial-gradient(1100px 650px at 78% 0%, rgba(125,211,252,0.13), transparent 62%), radial-gradient(900px 600px at 0% 100%, rgba(245,39,108,0.07), transparent 60%), ${THEME.deepest}` : undefined }}>
      <div style={{ position: "absolute", left: (width - 1920 * s) / 2, top: (height - 1080 * s) / 2, width: 1920, height: 1080, transform: `scale(${s})`, transformOrigin: "0 0", color: THEME.text, overflow: "hidden" }}>
        {background ? <WaveFloor sec={sec} /> : null}
        {children}
      </div>
    </AbsoluteFill>
  );
}

/** The app icon: five cyan bars on a rounded dark square. `live` makes the bars breathe like a voice. */
export function Logo({ size, sec = 0, live = 0, build = 1 }: { size: number; sec?: number; live?: number; build?: number }) {
  const bars = [0.35, 0.6, 0.85, 0.6, 0.35];
  return (
    <div style={{ width: size, height: size, borderRadius: size * 0.22, background: THEME.base, border: `${Math.max(1, size / 120)}px solid ${THEME.border}`, display: "flex", alignItems: "center", justifyContent: "center", gap: size * 0.04, boxShadow: "0 24px 70px rgba(0,0,0,0.5)" }}>
      {bars.map((h, i) => {
        const wobble = live * 0.25 * Math.sin(sec * 7 + i * 1.3) * Math.sin(sec * 2.3 + i);
        const grow = Math.min(1, Math.max(0, build * 1.6 - i * 0.15));
        return <div key={i} style={{ width: size * 0.07, height: size * Math.max(0.06, (h + wobble) * grow), background: THEME.wave }} />;
      })}
    </div>
  );
}

/** The pill from index.html, drawn at `scale`: light plus 32 bars, coloured by phase. */
export function Pill({ sec, phase = "recording", scale = 1 }: { sec: number; phase?: "recording" | "processing" | "done"; scale?: number }) {
  const color = phase === "processing" ? THEME.yellow : phase === "done" ? THEME.green : THEME.wave;
  const light = phase === "recording" ? THEME.red : color;
  return (
    <div style={{ width: 190 * scale, height: 40 * scale, borderRadius: 999, background: "rgba(18,18,22,0.94)", boxShadow: `0 ${8 * scale}px ${28 * scale}px rgba(0,0,0,0.45), inset 0 0 0 1px rgba(255,255,255,0.06)`, display: "flex", alignItems: "center", gap: 10 * scale, padding: `0 ${14 * scale}px 0 ${12 * scale}px`, boxSizing: "border-box" }}>
      <div style={{ width: 9 * scale, height: 9 * scale, borderRadius: "50%", background: light, boxShadow: `0 0 ${10 * scale}px ${2 * scale}px ${light}88`, flex: "none" }} />
      <div style={{ flex: 1, height: "100%", display: "flex", alignItems: "center", gap: 2.5 * scale }}>
        {Array.from({ length: 32 }, (_, i) => {
          const v = phase === "done" ? 0.06 : 0.12 + 0.75 * Math.abs(Math.sin(i * 0.71 - sec * 6) * Math.cos(i * 0.23 + sec * 1.7));
          return <div key={i} style={{ flex: 1, height: `${Math.max(5, v * 85)}%`, borderRadius: 99, background: color }} />;
        })}
      </div>
    </div>
  );
}

/** Top-left heading with a small cyan kicker. */
export function Heading({ kicker, text, show }: { kicker?: string; text: string; show: number }) {
  return (
    <div style={{ position: "absolute", left: 120, top: 84, opacity: show, transform: `translateX(${(1 - show) * -24}px)` }}>
      {kicker ? <div style={{ fontSize: 26, fontWeight: 700, letterSpacing: 4, textTransform: "uppercase", color: THEME.cyan, marginBottom: 12 }}>{kicker}</div> : null}
      <div style={{ fontSize: 68, fontWeight: 800, letterSpacing: -1.5 }}>{text}</div>
    </div>
  );
}
