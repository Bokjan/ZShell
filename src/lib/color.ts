/** A color as the color picker edits it: hue in degrees (0–360), saturation and brightness in percent. */
export interface Hsv {
  h: number;
  s: number;
  v: number;
}

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

/** `#rrggbb` in lowercase from what a user typed (`#abc`, `AABBCC`), or `null` if it isn't a color. */
export function parseHex(text: string): string | null {
  const hex = text.trim().replace(/^#/, "").toLowerCase();
  if (/^[0-9a-f]{6}$/.test(hex)) return `#${hex}`;
  if (/^[0-9a-f]{3}$/.test(hex)) return `#${[...hex].map((digit) => digit + digit).join("")}`;
  return null;
}

/** The hue, saturation and brightness of `#rrggbb`; black and grays have hue 0. */
export function hexToHsv(hex: string): Hsv {
  const value = parseInt((parseHex(hex) ?? "#000000").slice(1), 16);
  const [r, g, b] = [value >> 16, (value >> 8) & 0xff, value & 0xff].map((byte) => byte / 255);
  const max = Math.max(r, g, b);
  const delta = max - Math.min(r, g, b);
  let h = 0;
  if (delta > 0) {
    if (max === r) h = ((g - b) / delta + 6) % 6;
    else if (max === g) h = (b - r) / delta + 2;
    else h = (r - g) / delta + 4;
  }
  return { h: h * 60, s: max === 0 ? 0 : (delta / max) * 100, v: max * 100 };
}

/** `#rrggbb` for a hue, saturation and brightness, each clamped to its range. */
export function hsvToHex({ h, s, v }: Hsv): string {
  const hue = (clamp(h, 0, 360) % 360) / 60;
  const saturation = clamp(s, 0, 100) / 100;
  const brightness = clamp(v, 0, 100) / 100;
  const channel = (n: number) => {
    const k = (n + hue) % 6;
    return brightness - brightness * saturation * clamp(Math.min(k, 4 - k), 0, 1);
  };
  return `#${[channel(5), channel(3), channel(1)]
    .map((unit) =>
      Math.round(unit * 255)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}
