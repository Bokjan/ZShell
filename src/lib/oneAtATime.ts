/**
 * Runs `work` for events that come in bursts (a window being resized fires one each frame),
 * one run at a time: events while it runs ask for one more run once it is done, however many
 * of them come. Its answers so come back in order, and a burst costs two runs rather than one
 * for each event. Returns the function to call on each event.
 */
export function oneAtATime(work: () => Promise<unknown>): () => void {
  let running = false;
  let again = false;
  const done = () => {
    running = false;
    if (again) run();
  };
  const run = () => {
    if (running) {
      again = true;
      return;
    }
    running = true;
    again = false;
    void work().then(done, done);
  };
  return run;
}
