/**
 * What the terminal sends for programs rather than for the user: mouse (SGR, X10) and focus
 * reports, and answers to queries (device attributes, cursor position, status, modes, window
 * size, OSC colors, DCS). Each comes as one `onData`. A cursor position report can't be told
 * apart from Shift+F3 (`ESC[1;2R`), which counts as one too.
 */
const REPORT =
  /^\x1b(?:\[(?:<\d+;\d+;\d+[Mm]|M[\s\S]{3}|I|O|[?>]?[\d;]*c|\??\d+;\d+(?:\$y|R)|\d*n|[\d;]*t)|[\]P][\s\S]*(?:\x1b\\|\x07))$/;

export const isTerminalReport = (data: string) => REPORT.test(data);
