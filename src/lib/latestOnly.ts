/**
 * Answers to a question asked again and again (a status, read again as events come), which
 * can come back out of order: only the answer to the latest question counts, and a change made
 * here meanwhile (`changed`) outdates the questions asked before it. Otherwise an answer read
 * before the app locked could uncover the window after the lock screen had covered it.
 */
export function latestOnly<T>(apply: (value: T) => void) {
  let latest = 0;
  return {
    /** Applies what `answer` resolves to, unless a newer one was asked or a change made since; resolves to it either way. */
    track(answer: Promise<T>): Promise<T> {
      const request = ++latest;
      return answer.then((value) => {
        if (request === latest) apply(value);
        return value;
      });
    },
    /** Applies `value`, a change made here: answers to the questions asked before it don't count. */
    changed(value: T) {
      latest++;
      apply(value);
    },
  };
}
