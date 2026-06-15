/** Interpolation alignée sur la légende carte (bleu → vert → jaune). */

export function pressureToCss(pressure: number, minP: number, maxP: number): string {
  const span = Math.max(maxP - minP, 0.01);
  const t = Math.min(1, Math.max(0, (pressure - minP) / span));
  const stops = [
    { t: 0, r: 0x1e, g: 0x88, b: 0xe5 },
    { t: 0.5, r: 0x43, g: 0xa0, b: 0x47 },
    { t: 1, r: 0xfb, g: 0xc0, b: 0x2d },
  ];
  let i = 0;
  while (i < stops.length - 1 && t > stops[i + 1].t) i += 1;
  const a = stops[i];
  const b = stops[Math.min(i + 1, stops.length - 1)];
  const local = b.t === a.t ? 0 : (t - a.t) / (b.t - a.t);
  const r = Math.round(a.r + (b.r - a.r) * local);
  const g = Math.round(a.g + (b.g - a.g) * local);
  const bl = Math.round(a.b + (b.b - a.b) * local);
  return `rgb(${r}, ${g}, ${bl})`;
}

export function pressureRange(values: number[]): { min: number; max: number } {
  if (values.length === 0) return { min: 0, max: 1 };
  return { min: Math.min(...values), max: Math.max(...values) };
}
