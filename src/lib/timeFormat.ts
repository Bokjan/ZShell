import { DEFAULT_SETTINGS, DEFAULT_TIME_FORMAT_PARTS, logs, type TimePart } from "../ipc/api";
import { currentSettings, settingsStore } from "./settings";

/** The settings' time format, as the backend parsed it. */
let parsed = { format: DEFAULT_SETTINGS.logs.timeFormat, parts: DEFAULT_TIME_FORMAT_PARTS };

/**
 * The parts of the settings' time format (`logs.timeFormat`), which our own messages in the
 * terminal start with: the default ones until the backend, which alone parses and checks
 * formats, has parsed the settings' one (see `followTimeFormat`).
 */
export function timeFormatParts(): TimePart[] {
  return parsed.parts;
}

/**
 * Has the settings' time format parsed now and whenever it changes, so that the first message
 * is in it too. Returns what stops following.
 */
export function followTimeFormat(): () => void {
  const unsubscribe = settingsStore.subscribe(follow);
  follow();
  return unsubscribe;
}

function follow() {
  const format = currentSettings().logs.timeFormat;
  if (format === parsed.format) return;
  logs
    .timeFormat(format)
    // Unless the setting changed again meanwhile.
    .then((parts) => {
      if (currentSettings().logs.timeFormat === format) parsed = { format, parts };
    })
    .catch(console.error);
}
