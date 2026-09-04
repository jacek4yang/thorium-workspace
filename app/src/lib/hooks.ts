// Small shared React hooks. Nothing here talks to the backend directly.

import { useCallback, useRef, useState } from "react";

export interface Toast {
  id: number;
  message: string;
  tone: "success" | "error";
}

const TOAST_TIMEOUT_MS = 5000;

/**
 * A tiny toast queue. Success toasts disappear on their own; error toasts
 * stay until dismissed because they usually need a decision.
 */
export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(1);

  const dismiss = useCallback((id: number) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const push = useCallback(
    (message: string, tone: Toast["tone"] = "success") => {
      const id = nextId.current++;
      setToasts((current) => [...current.slice(-4), { id, message, tone }]);
      if (tone === "success") {
        window.setTimeout(() => dismiss(id), TOAST_TIMEOUT_MS);
      }
    },
    [dismiss],
  );

  return { toasts, push, dismiss };
}

export type ToastFn = ReturnType<typeof useToasts>["push"];
