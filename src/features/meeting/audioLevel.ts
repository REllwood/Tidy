/** Map linear PCM RMS to a useful speech meter range (-60 dBFS to 0 dBFS). */
export function audioLevel(rms: number): number {
  if (!Number.isFinite(rms) || rms <= 0.001) return 0;
  return Math.max(
    0,
    Math.min(1, (20 * Math.log10(Math.min(rms, 1)) + 60) / 60),
  );
}
