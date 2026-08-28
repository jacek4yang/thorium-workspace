/**
 * Shared hooks.
 *
 * The toast store lives here rather than in a context provider because it is
 * used from every page and never needs to re-render a tree that does not
 * subscribe to it.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { toAppError } from "./api";
import type { AppError } from "./types";

export interface Toast {
  id: number;
  message: string;
  tone: "info" | "error";
}

let nextToastId = 1;

/** A tiny toast queue with automatic expiry. */
export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const timers = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map());

  const dismiss = useCallback((id: number) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
    const timer = timers.current.get(id);
    if (timer !== undefined) {
      clearTimeout(timer);
      timers.current.delete(id);
    }
  }, []);

  const push = useCallback(
    (message: string, tone: Toast["tone"] = "info") => {
      const id = nextToastId++;
      setToasts((current) => [...current, { id, message, tone }]);
      // Errors linger: the user may need to read a diagnostic code off them.
      const timer = setTimeout(() => dismiss(id), tone === "error" ? 8000 : 3200);
      timers.current.set(id, timer);
    },
    [dismiss],
  );

  useEffect(() => {
    const pending = timers.current;
    return () => {
      pending.forEach(clearTimeout);
      pending.clear();
    };
  }, []);

  return { toasts, push, dismiss };
}

/** Async state for one request. */
export interface AsyncState<T> {
  data: T | null;
  error: AppError | null;
  loading: boolean;
}

/**
 * Runs `loader` on mount, whenever `deps` change, and whenever `reload` is
 * called.
 *
 * `loading` is derived rather than stored, so nothing is set synchronously
 * inside the effect, and a result from a superseded run is discarded: a fast
 * second navigation cannot overwrite the newer view with older data.
 */
export function useAsync<T>(loader: () => Promise<T>, deps: unknown[] = []) {
  const [result, setResult] = useState<{ data: T | null; error: AppError | null; run: number }>({
    data: null,
    error: null,
    run: -1,
  });
  const [run, setRun] = useState(0);
  const loaderRef = useRef(loader);

  // Callers rebuild the closure every render; the dependency list is what
  // decides when it is actually called.
  useEffect(() => {
    loaderRef.current = loader;
  });

  useEffect(() => {
    let cancelled = false;
    loaderRef
      .current()
      .then((data) => {
        if (!cancelled) setResult({ data, error: null, run });
      })
      .catch((error: unknown) => {
        if (!cancelled) setResult({ data: null, error: toAppError(error), run });
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [run, ...deps]);

  const reload = useCallback(() => setRun((value) => value + 1), []);
  const setData = useCallback(
    (data: T) => setResult((current) => ({ data, error: null, run: current.run })),
    [],
  );

  return {
    data: result.data,
    error: result.error,
    loading: result.run !== run,
    reload,
    setData,
  };
}

/** Re-renders once per second, for live countdowns. */
export function useTicker(active: boolean) {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!active) return;
    const timer = setInterval(() => setTick((value) => value + 1), 1000);
    return () => clearInterval(timer);
  }, [active]);
  return tick;
}
