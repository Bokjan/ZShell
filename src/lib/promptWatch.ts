import { Timer } from "./disposables";

/**
 * How long the output pauses after a command is entered before the shell counts as back at its
 * prompt, maybe in another folder (which the file panel may follow).
 */
export const PROMPT_SETTLE_MS = 300;

/**
 * Once a command is entered (Enter, or a paste with a line break), the next pause in the
 * output tells (`settled`) that the shell may have changed folders.
 */
export class PromptWatch {
  private entered = false;
  private readonly timer = new Timer();

  constructor(private readonly settled: () => void) {}

  /** What was typed or pasted (not the terminal's reports to the program). */
  typed(data: string) {
    if (!data.includes("\r")) return;
    this.entered = true;
    this.wait();
  }

  /** The terminal has taken in output. */
  output() {
    if (this.entered) this.wait();
  }

  dispose() {
    this.timer.dispose();
  }

  private wait() {
    this.timer.start(() => {
      this.entered = false;
      this.settled();
    }, PROMPT_SETTLE_MS);
  }
}
