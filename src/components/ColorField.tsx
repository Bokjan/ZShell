import { useEffect, useEffectEvent, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type RefObject } from "react";
import { useTranslation } from "react-i18next";

import { errorMessage, pickColor } from "../ipc/api";
import { announce } from "../lib/announce";
import { attempt } from "../lib/attempt";
import { hexToHsv, hsvToHex, parseHex, type Hsv } from "../lib/color";
import { isComposing } from "../lib/platform";
import { ErrorText } from "./ErrorMessage";
import { IconButton } from "./IconButton";
import { EyedropperIcon } from "./icons";

interface Props {
  /** `#rrggbb`. */
  value: string;
  onChange(value: string): void;
  /** Names the field and its popup. */
  label: string;
  disabled?: boolean;
}

/**
 * A color field: a swatch that opens a popup of the app's own, with an area for saturation
 * and brightness, a hue bar, the color in hex, and an eyedropper that picks a color from the
 * screen (`color_pick`). It replaces `<input type="color">`, whose popup is each platform's
 * and whose eyedropper does nothing in WebView2 (see `color_pick` in the backend).
 */
export function ColorField({ value, onChange, label, disabled }: Props) {
  const { t } = useTranslation();
  const swatch = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        ref={swatch}
        type="button"
        className="color-swatch"
        style={{ background: value }}
        aria-label={t("colorPicker.swatch", { label, color: value })}
        aria-haspopup="dialog"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => setOpen((open) => !open)}
      />
      {open && !disabled && <ColorPopup anchor={swatch} value={value} label={label} onChange={onChange} onClose={() => setOpen(false)} />}
    </>
  );
}

const MARGIN = 8;

interface PopupProps {
  anchor: RefObject<HTMLButtonElement | null>;
  value: string;
  label: string;
  onChange(value: string): void;
  onClose(): void;
}

/**
 * Below the swatch (above it when there is no room), in the dialog's own tree so that Tab
 * goes through it, and placed in the window so that the dialog's scrolling doesn't clip it.
 * Closes on Escape (which the dialog then doesn't get), a press outside it, the focus moving
 * out of it, or the dialog scrolling or the window resizing under it.
 */
function ColorPopup({ anchor, value, label, onChange, onClose }: PopupProps) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  const area = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);
  // The hue, saturation and brightness last set here: a gray has no hue of its own, which the
  // hue bar keeps, as long as the value is still the color they make.
  const [hsv, setHsv] = useState(() => hexToHsv(value));
  const shown = hsvToHex(hsv) === value ? hsv : hexToHsv(value);
  // The hex field while it is being edited, which may not be a color yet.
  const [typed, setTyped] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const onCloseEvent = useEffectEvent(onClose);

  useLayoutEffect(() => {
    const popup = ref.current!;
    const swatch = anchor.current!.getBoundingClientRect();
    const { width, height } = popup.getBoundingClientRect();
    const below = swatch.bottom + 4;
    const top = below + height <= window.innerHeight - MARGIN ? below : swatch.top - 4 - height;
    setPosition({
      left: Math.max(MARGIN, Math.min(swatch.right - width, window.innerWidth - width - MARGIN)),
      top: Math.max(MARGIN, top),
    });
  }, [anchor]);

  // Once placed: it can't take the focus while hidden.
  const placed = position !== null;
  useEffect(() => {
    if (placed) area.current?.focus({ preventScroll: true });
  }, [placed]);

  useEffect(() => {
    const close = () => onCloseEvent();
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      // The swatch closes it itself.
      if (!ref.current?.contains(target) && !anchor.current?.contains(target)) close();
    };
    const onScroll = (e: Event) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    window.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", close);
    };
  }, [anchor]);

  const set = (next: Hsv) => {
    setHsv(next);
    setTyped(null);
    onChange(hsvToHex(next));
  };
  const setHex = (hex: string) => {
    setHsv(hexToHsv(hex));
    onChange(hex);
  };
  const closeToSwatch = () => {
    onClose();
    anchor.current?.focus();
  };

  const fromPointer = (e: PointerEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const x = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
    const y = Math.min(1, Math.max(0, (e.clientY - rect.top) / rect.height));
    return { x, y };
  };
  const onAreaPointer = (e: PointerEvent<HTMLDivElement>) => {
    if (e.type === "pointerdown") {
      e.currentTarget.setPointerCapture(e.pointerId);
      e.currentTarget.focus();
    } else if (!e.currentTarget.hasPointerCapture(e.pointerId)) {
      return;
    }
    const { x, y } = fromPointer(e);
    set({ ...shown, s: x * 100, v: (1 - y) * 100 });
  };
  const onHuePointer = (e: PointerEvent<HTMLDivElement>) => {
    if (e.type === "pointerdown") {
      e.currentTarget.setPointerCapture(e.pointerId);
      e.currentTarget.focus();
    } else if (!e.currentTarget.hasPointerCapture(e.pointerId)) {
      return;
    }
    set({ ...shown, h: fromPointer(e).x * 360 });
  };

  const step = (e: KeyboardEvent) => (e.shiftKey ? 10 : 1);
  const onAreaKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const moves: Record<string, Partial<Hsv>> = {
      ArrowLeft: { s: shown.s - step(e) },
      ArrowRight: { s: shown.s + step(e) },
      ArrowDown: { v: shown.v - step(e) },
      ArrowUp: { v: shown.v + step(e) },
    };
    const move = moves[e.key];
    if (!move) return;
    e.preventDefault();
    const next = { ...shown, ...move };
    set({ ...next, s: Math.min(100, Math.max(0, next.s)), v: Math.min(100, Math.max(0, next.v)) });
  };
  const onHueKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const hues: Record<string, number> = {
      ArrowLeft: shown.h - step(e),
      ArrowDown: shown.h - step(e),
      ArrowRight: shown.h + step(e),
      ArrowUp: shown.h + step(e),
      PageDown: shown.h - 10,
      PageUp: shown.h + 10,
      Home: 0,
      End: 360,
    };
    const h = hues[e.key];
    if (h === undefined) return;
    e.preventDefault();
    set({ ...shown, h: Math.min(360, Math.max(0, h)) });
  };

  const pick = () => {
    setError(null);
    setPicking(true);
    void attempt(
      async () => {
        const color = await pickColor();
        if (color) {
          setTyped(null);
          setHex(color);
          announce(t("colorPicker.picked", { color }));
        }
      },
      { onError: (e) => setError(errorMessage(e)), cleanup: () => setPicking(false) },
    );
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (isComposing(e)) return;
    // Escape closes the popup rather than the dialog; Enter would submit the dialog's form.
    if (e.key === "Escape" || (e.key === "Enter" && e.target instanceof HTMLInputElement)) {
      e.preventDefault();
      closeToSwatch();
    }
  };

  const saturation = Math.round(shown.s);
  const brightness = Math.round(shown.v);
  const hue = Math.round(shown.h);
  return (
    <div
      ref={ref}
      className="color-popup"
      role="dialog"
      aria-label={label}
      style={position ? { left: position.left, top: position.top } : { visibility: "hidden" }}
      // WebKit on macOS doesn't focus a button that is clicked but the nearest focusable
      // element around it: this popup rather than the dialog, so that Escape still reaches it.
      tabIndex={-1}
      onKeyDown={onKeyDown}
      onBlur={(e) => {
        // Not when the window loses the focus (`relatedTarget` is null), as it does to the
        // eyedropper, nor when it goes to an element around the popup, which is a click on
        // the swatch there (a press elsewhere closes it by itself): closing on the press would
        // have the swatch's click open it again.
        const next = e.relatedTarget as Node | null;
        if (next && !ref.current?.contains(next) && !anchor.current?.contains(next) && !next.contains(ref.current)) onClose();
      }}
    >
      <div
        ref={area}
        className="color-area"
        style={{ backgroundColor: `hsl(${shown.h} 100% 50%)` }}
        role="slider"
        tabIndex={0}
        aria-label={t("colorPicker.area")}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={saturation}
        aria-valuetext={t("colorPicker.areaValue", { saturation, brightness })}
        onPointerDown={onAreaPointer}
        onPointerMove={onAreaPointer}
        onKeyDown={onAreaKey}
      >
        <span className="color-thumb" style={{ left: `${shown.s}%`, top: `${100 - shown.v}%`, background: value }} />
      </div>
      <div
        className="color-hue"
        role="slider"
        tabIndex={0}
        aria-label={t("colorPicker.hue")}
        aria-valuemin={0}
        aria-valuemax={360}
        aria-valuenow={hue}
        onPointerDown={onHuePointer}
        onPointerMove={onHuePointer}
        onKeyDown={onHueKey}
      >
        <span className="color-thumb" style={{ left: `${(shown.h / 360) * 100}%`, background: `hsl(${shown.h} 100% 50%)` }} />
      </div>
      <div className="color-popup-row">
        <label className="color-hex">
          {t("colorPicker.hex")}
          <input
            value={typed ?? value}
            onChange={(e) => {
              setTyped(e.target.value);
              const hex = parseHex(e.target.value);
              if (hex) setHex(hex);
            }}
            onBlur={() => setTyped(null)}
            autoCapitalize="off"
            autoCorrect="off"
            maxLength={7}
          />
        </label>
        <IconButton type="button" className="icon-button" label={t("colorPicker.pick")} disabled={picking} onClick={pick}>
          <EyedropperIcon />
        </IconButton>
      </div>
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
