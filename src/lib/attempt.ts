/**
 * `try { return await body() } catch (e) { return onError(e) } finally { cleanup() }`, for
 * components and hooks. React Compiler can't compile some `try` statements yet (with `finally`,
 * without `catch`, or holding a conditional, `?.` or `??`), and then skips the whole component
 * even when the statement is only in one of its event handlers. Without `onError`, an error
 * goes on to the caller after `cleanup`, as from `try … finally`.
 *
 * A `return` in `body` or `onError` leaves only that function: code after the call still runs.
 */
export async function attempt<T>(
  body: () => Promise<T>,
  { onError, cleanup }: { onError?: (error: unknown) => T | Promise<T>; cleanup?: () => void },
): Promise<T> {
  try {
    return await body();
  } catch (error) {
    if (!onError) throw error;
    return await onError(error);
  } finally {
    cleanup?.();
  }
}

/** `attempt` for work that doesn't wait: `try { return body() } finally { cleanup() }`. */
export function attemptSync<T>(body: () => T, cleanup: () => void): T {
  try {
    return body();
  } finally {
    cleanup();
  }
}
