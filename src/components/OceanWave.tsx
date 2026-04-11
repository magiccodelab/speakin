import { useEffect, useRef } from "react";
import { cn } from "../lib/utils";

/**
 * Ambient ocean-wave visualization.
 *
 * Two layered filled "ribbons" (closed sine curves with constant thickness)
 * flow across the full width of the container, with amplitude softly
 * modulated by a shared audio level ref. Rendered as SVG, but paths are
 * mutated via `setAttribute("d", …)` from a single RAF loop — the component
 * never triggers a React re-render during animation. Only opacity / color /
 * active flag come through normal React props.
 *
 * Tuning is scaled by `viewHeight / REFERENCE_HEIGHT` so the wave fills the
 * available vertical space proportionally at any host size without clipping.
 */

// Internal viewBox width. `preserveAspectRatio="none"` stretches this out
// to the container's actual width, so the sample count is fixed regardless
// of the final rendered width.
const WAVE_VIEW_W = 300;
const WAVE_SAMPLES = 56;

// All amp / thickness ranges below are tuned for this reference height.
// The component scales them by `viewHeight / REFERENCE_HEIGHT`.
const REFERENCE_HEIGHT = 40;

interface OceanWaveProps {
  /**
   * Shared audio level ref (0..1). The RAF loop reads from this every
   * frame; the parent's audio-level listener is responsible for writing.
   * Passing a ref (rather than a React state value) keeps the animation
   * decoupled from React's render cycle.
   */
  levelRef: React.MutableRefObject<number>;
  /**
   * When false, the internal RAF loop pauses. Pair this with an opacity-0
   * CSS class to fully hide the component.
   */
  active: boolean;
  /** CSS color value for both ribbon fills, e.g. "hsl(var(--primary))". */
  color: string;
  /** Extra classes for sizing / positioning (width, position, opacity). */
  className?: string;
  /**
   * Internal viewBox height. Used for both SVG scaling and amp/thickness
   * proportional scaling. Default 40 (main-page reference).
   */
  viewHeight?: number;
}

/**
 * Build a closed SVG path representing a constant-thickness sine ribbon:
 * the top edge is walked left→right following `cy + amp·sin(…) − thickness/2`,
 * the bottom edge then walked right→left at `+ thickness/2`, then closed.
 * The result is a smooth filled wavy band.
 */
function buildRibbonPath(
  cycles: number,
  phase: number,
  amp: number,
  thickness: number,
  height: number,
): string {
  const centerY = height / 2;
  const step = WAVE_VIEW_W / WAVE_SAMPLES;
  const halfT = thickness / 2;

  let d = "";
  // Top edge — left to right.
  for (let i = 0; i <= WAVE_SAMPLES; i++) {
    const x = i * step;
    const angle = (i / WAVE_SAMPLES) * Math.PI * 2 * cycles + phase;
    const y = centerY + amp * Math.sin(angle);
    d += `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${(y - halfT).toFixed(2)} `;
  }
  // Bottom edge — right to left.
  for (let i = WAVE_SAMPLES; i >= 0; i--) {
    const x = i * step;
    const angle = (i / WAVE_SAMPLES) * Math.PI * 2 * cycles + phase;
    const y = centerY + amp * Math.sin(angle);
    d += `L ${x.toFixed(1)} ${(y + halfT).toFixed(2)} `;
  }
  d += "Z";
  return d;
}

export function OceanWave({
  levelRef,
  active,
  color,
  className,
  viewHeight = REFERENCE_HEIGHT,
}: OceanWaveProps) {
  const path1Ref = useRef<SVGPathElement>(null);
  const path2Ref = useRef<SVGPathElement>(null);

  useEffect(() => {
    if (!active) return;
    let running = true;

    // Offset the second phase by ~quarter cycle so the two layers don't
    // crest together — prevents the ribbon looking like one fat line.
    let phase1 = 0;
    let phase2 = Math.PI * 0.7;
    let smooth = 0;
    let last = performance.now();
    let raf = 0;

    // Scale all amp/thickness ranges so the ribbon fills `viewHeight`
    // proportionally regardless of host container size.
    const scale = viewHeight / REFERENCE_HEIGHT;

    const tick = (now: number) => {
      if (!running) return;
      // Clamp dt so a backgrounded tab doesn't explode phase on resume.
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;

      // Asymmetric smoothing: slower than a per-bar equalizer so the
      // ribbon reads as a "settling pond" rather than a jittery meter.
      const target = levelRef.current;
      const alpha = target > smooth ? 0.22 : 0.07;
      smooth += (target - smooth) * alpha;

      // Slightly different phase speeds → subtle parallax.
      phase1 += dt * 1.1;
      phase2 += dt * 1.7;

      // Layer 1 (back): larger swing, softer fill.
      const amp1 = (4 + smooth * 11) * scale;
      const thk1 = (3 + smooth * 3) * scale;
      // Layer 2 (front): tighter, thinner, stronger fill.
      const amp2 = (3 + smooth * 8) * scale;
      const thk2 = (2 + smooth * 2.5) * scale;

      if (path1Ref.current) {
        path1Ref.current.setAttribute(
          "d",
          buildRibbonPath(2.2, phase1, amp1, thk1, viewHeight),
        );
      }
      if (path2Ref.current) {
        path2Ref.current.setAttribute(
          "d",
          buildRibbonPath(3.6, phase2, amp2, thk2, viewHeight),
        );
      }

      raf = requestAnimationFrame(tick);
    };

    raf = requestAnimationFrame(tick);
    return () => {
      running = false;
      cancelAnimationFrame(raf);
    };
  }, [active, levelRef, viewHeight]);

  return (
    <svg
      aria-hidden
      viewBox={`0 0 ${WAVE_VIEW_W} ${viewHeight}`}
      preserveAspectRatio="none"
      className={cn("pointer-events-none", className)}
    >
      {/* Back layer: larger & softer */}
      <path ref={path1Ref} fill={color} fillOpacity="0.32" />
      {/* Front layer: tighter & stronger — overlapped areas read slightly
          darker, producing a natural depth feel without extra gradients. */}
      <path ref={path2Ref} fill={color} fillOpacity="0.55" />
    </svg>
  );
}
