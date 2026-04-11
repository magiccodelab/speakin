/**
 * UI sound playback for recording start/stop/error events.
 *
 * Two flavors coexist:
 *
 * 1. "默认" — a tiny procedural synth option per event, generated at runtime
 *    via the Web Audio API. Ported from the earliest version of this file
 *    (git `3a9f6c8` / `ca17cae`) so the default sonic identity is stable
 *    and requires zero bundled assets. Acts as a safe fallback that always
 *    plays, even if the wav catalog is missing.
 *
 * 2. "声音 1..N" — static .wav files under `public/sounds/{event}/NN.wav`,
 *    which Vite copies to the app bundle as-is.
 *
 * Users pick one option per event in settings; the selected id is persisted
 * by Rust and synced back into this module via `setCurrentSoundPresets`.
 */

export type SoundEvent = "start" | "stop" | "error";

interface BaseSoundOption {
  /** Unique id, globally unique across events. Stable across versions. */
  id: string;
  /** Human-readable name shown in the picker. */
  name: string;
  /** One-line description shown in the dropdown label. */
  description: string;
}

/** File-backed option — plays a .wav from `public/sounds/…`. */
export interface FileSoundOption extends BaseSoundOption {
  kind: "file";
  /** URL relative to the site root, e.g. "/sounds/start/01.wav". */
  url: string;
}

/** Procedural option — synthesized at runtime with the Web Audio API. */
export interface SynthSoundOption extends BaseSoundOption {
  kind: "synth";
  /** Which synth routine to run. */
  event: SoundEvent;
}

export type SoundOption = FileSoundOption | SynthSoundOption;

// ── Catalog ──
//
// Each event has: one procedural "默认" option first, followed by every wav
// file in that folder. Folder counts are hard-coded so the module stays
// zero-runtime-cost; update these when files are added or removed.

const START_COUNT = 9;
const STOP_COUNT = 7;
const ERROR_COUNT = 8;

const SYNTH_OPTIONS: Record<SoundEvent, SynthSoundOption> = {
  start: { id: "default-start", name: "默认", description: "程序生成的轻量音效", kind: "synth", event: "start" },
  stop:  { id: "default-stop",  name: "默认", description: "程序生成的轻量音效", kind: "synth", event: "stop"  },
  error: { id: "default-error", name: "默认", description: "程序生成的轻量音效", kind: "synth", event: "error" },
};

function buildOptions(event: SoundEvent, count: number): SoundOption[] {
  const opts: SoundOption[] = [SYNTH_OPTIONS[event]];
  for (let i = 1; i <= count; i++) {
    const n = i.toString().padStart(2, "0");
    opts.push({
      id: `${event}-${n}`,
      name: `声音 ${i}`,
      description: "",
      kind: "file",
      url: `/sounds/${event}/${n}.wav`,
    });
  }
  return opts;
}

/** Full catalog keyed by event. Exported for the settings picker. */
export const SOUND_OPTIONS: Record<SoundEvent, SoundOption[]> = {
  start: buildOptions("start", START_COUNT),
  stop: buildOptions("stop", STOP_COUNT),
  error: buildOptions("error", ERROR_COUNT),
};

/** Default option id per event. Used when a saved id is missing or invalid. */
export const DEFAULT_SOUND_OPTION_IDS: Record<SoundEvent, string> = {
  start: "default-start",
  stop: "default-stop",
  error: "default-error",
};

function resolveOption(event: SoundEvent, id: string): SoundOption {
  const list = SOUND_OPTIONS[event];
  return list.find((o) => o.id === id) ?? list[0];
}

// ── File playback ──
//
// We cache one HTMLAudioElement per URL so repeated plays don't re-decode the
// file. `currentTime = 0` + `play()` is enough to restart an in-flight sound,
// which is fine here since recording start/stop/error events never fire fast
// enough to need overlapping playback.

const audioCache = new Map<string, HTMLAudioElement>();

function getAudio(url: string): HTMLAudioElement {
  let audio = audioCache.get(url);
  if (!audio) {
    audio = new Audio(url);
    audio.preload = "auto";
    audioCache.set(url, audio);
  }
  return audio;
}

function playUrl(url: string) {
  try {
    const audio = getAudio(url);
    audio.currentTime = 0;
    // play() returns a promise that may reject if the browser hasn't seen a
    // user gesture yet. In this app every playback is triggered by a hotkey
    // or click, so rejections are rare — swallow them to avoid console noise.
    void audio.play().catch(() => {});
  } catch {
    // Fail silently: an inaudible cue is better than a thrown error.
  }
}

// ── Procedural synth playback ──
//
// Ported verbatim from the earliest revisions of this file. Intentionally
// simple — a single sine with an exponential fade and a few delayed layers.
// Do not "modernize" this: the value is that it exactly matches the original
// default cue users have heard since v1.

let synthCtx: AudioContext | null = null;

function getSynthContext(): AudioContext {
  if (!synthCtx) synthCtx = new AudioContext();
  return synthCtx;
}

/** Play a soft sine tone with a linear-to-exponential fade. */
function playSynthTone(freq: number, duration: number, volume: number, delay = 0) {
  const ac = getSynthContext();
  const start = ac.currentTime + delay;
  const osc = ac.createOscillator();
  const gain = ac.createGain();
  osc.type = "sine";
  osc.frequency.value = freq;
  gain.gain.setValueAtTime(volume, start);
  // Exponential ramp to 0.001 (can't ramp to exactly 0) gives a natural tail.
  gain.gain.exponentialRampToValueAtTime(0.001, start + duration);
  osc.connect(gain);
  gain.connect(ac.destination);
  osc.start(start);
  osc.stop(start + duration + 0.03);
}

function playSynth(event: SoundEvent) {
  try {
    const ac = getSynthContext();
    // Cache one "now" per invocation so every schedule point is relative to
    // the same t0. Without this, each ac.currentTime read drifts by a few
    // microseconds of JS execution time, desynchronizing layered tones.
    const t0 = ac.currentTime;
    switch (event) {
      case "start": {
        // Ascending C5 → E5 + a subtle E6 shimmer overtone. Verbatim from
        // git 3a9f6c8 — including the deliberately quirky shimmer timing.
        playSynthTone(523, 0.15, 0.10);
        playSynthTone(659, 0.20, 0.10, 0.08);

        // Shimmer: gain starts ramping from 0.03 at t0, but the oscillator
        // doesn't fire until t0+0.08. By the time you actually hear it, the
        // exponential ramp has already decayed to ~0.0097, so the shimmer
        // is VERY soft — a breath of upper air rather than a separate note.
        // If you "fix" this by moving setValueAtTime to t0+0.08 the shimmer
        // will be ~3× louder, which is not what the original sounded like.
        const shimmer = ac.createOscillator();
        const shimmerGain = ac.createGain();
        shimmer.type = "sine";
        shimmer.frequency.value = 1318;
        shimmerGain.gain.setValueAtTime(0.03, t0);
        shimmerGain.gain.exponentialRampToValueAtTime(0.001, t0 + 0.25);
        shimmer.connect(shimmerGain);
        shimmerGain.connect(ac.destination);
        shimmer.start(t0 + 0.08);
        shimmer.stop(t0 + 0.3);
        break;
      }
      case "stop": {
        // Softer, shorter descending G5 → C5.
        playSynthTone(784, 0.18, 0.08);
        playSynthTone(523, 0.22, 0.06, 0.07);
        break;
      }
      case "error": {
        // Short low descending warning tone.
        playSynthTone(165, 0.20, 0.15);
        playSynthTone(131, 0.25, 0.12, 0.10);
        break;
      }
    }
  } catch {
    // Fail silently.
  }
}

// ── Dispatch ──

function playOption(opt: SoundOption) {
  if (opt.kind === "synth") {
    playSynth(opt.event);
  } else {
    playUrl(opt.url);
  }
}

// ── Public API ──

/**
 * Per-event currently active option ids. Each recording event can pick
 * its own option independently. Mutated via `setCurrentSoundPresets`.
 */
const currentOptionIds: Record<SoundEvent, string> = { ...DEFAULT_SOUND_OPTION_IDS };

/**
 * Update the active options used by `playStartSound` / `playStopSound` /
 * `playErrorSound`. Call this from the settings sync effect whenever
 * any of `sound_preset_start/stop/error` changes. Only provided keys
 * are updated, so partial updates are safe.
 */
export function setCurrentSoundPresets(presets: Partial<Record<SoundEvent, string>>) {
  for (const key of Object.keys(presets) as SoundEvent[]) {
    const id = presets[key];
    if (id) currentOptionIds[key] = id;
  }
}

/**
 * Play a specific option for a specific event, without touching the
 * module's "current" selection. Used by the settings preview so
 * auditioning an option doesn't mutate global state.
 */
export function playSound(optionId: string, event: SoundEvent) {
  playOption(resolveOption(event, optionId));
}

/** Recording started — uses the option configured for the `start` event. */
export function playStartSound() {
  playOption(resolveOption("start", currentOptionIds.start));
}

/** Recording stopped — uses the option configured for the `stop` event. */
export function playStopSound() {
  playOption(resolveOption("stop", currentOptionIds.stop));
}

/** Error occurred — uses the option configured for the `error` event. */
export function playErrorSound() {
  playOption(resolveOption("error", currentOptionIds.error));
}
