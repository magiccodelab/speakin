/**
 * Soft synthetic sound effects using Web Audio API.
 * No audio files needed — tones are generated procedurally.
 */

let ctx: AudioContext | null = null;

function getContext(): AudioContext {
  if (!ctx) ctx = new AudioContext();
  return ctx;
}

/** Play a soft tone with given frequency, duration and volume curve. */
function playTone(freq: number, duration: number, type: OscillatorType = "sine", volume = 0.12) {
  const ac = getContext();
  const osc = ac.createOscillator();
  const gain = ac.createGain();

  osc.type = type;
  osc.frequency.value = freq;
  gain.gain.setValueAtTime(volume, ac.currentTime);
  // Soft fade out to avoid clicks
  gain.gain.exponentialRampToValueAtTime(0.001, ac.currentTime + duration);

  osc.connect(gain);
  gain.connect(ac.destination);
  osc.start(ac.currentTime);
  osc.stop(ac.currentTime + duration);
}

/** Recording started — soft ascending two-note chime. */
export function playStartSound() {
  const ac = getContext();
  // Note 1: C5 (523Hz)
  playTone(523, 0.15, "sine", 0.10);
  // Note 2: E5 (659Hz) slightly delayed
  setTimeout(() => {
    playTone(659, 0.2, "sine", 0.10);
  }, 80);

  // Subtle shimmer overtone
  const osc = ac.createOscillator();
  const gain = ac.createGain();
  osc.type = "sine";
  osc.frequency.value = 1318; // E6 - octave above
  gain.gain.setValueAtTime(0.03, ac.currentTime);
  gain.gain.exponentialRampToValueAtTime(0.001, ac.currentTime + 0.25);
  osc.connect(gain);
  gain.connect(ac.destination);
  osc.start(ac.currentTime + 0.08);
  osc.stop(ac.currentTime + 0.3);
}

/** Error occurred — short low descending warning tone. */
export function playErrorSound() {
  playTone(165, 0.2, "sine", 0.15);
  setTimeout(() => playTone(131, 0.25, "sine", 0.12), 100);
}

/** Recording stopped — soft descending single note. */
export function playStopSound() {
  // G5 → fade (softer, shorter than start)
  playTone(784, 0.18, "sine", 0.08);
  setTimeout(() => {
    playTone(523, 0.22, "sine", 0.06);
  }, 70);
}
