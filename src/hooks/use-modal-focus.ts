import { useEffect, useRef, type RefObject } from "react";

/** Elements that participate in the modal tab order. */
export const MODAL_FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "[href]",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

interface ModalFocusOptions {
  open: boolean;
  panelRef: RefObject<HTMLElement | null>;
  initialFocusRef?: RefObject<HTMLElement | null>;
  onClose: () => void;
  /** When false, Escape is ignored (e.g. a write is in progress). */
  canClose?: () => boolean;
}

function focusables(panel: HTMLElement): HTMLElement[] {
  return Array.from(
    panel.querySelectorAll<HTMLElement>(MODAL_FOCUSABLE_SELECTOR),
  );
}

/**
 * Shared modal behavior: initial focus, Tab/Shift+Tab focus trap, Escape to
 * close and focus restore to the element that opened the modal.
 *
 * Used by every modal (install wizard, update, uninstall, detach, anomaly and
 * exit confirmation) so the keyboard contract cannot drift between dialogs.
 */
export function useModalFocus({
  open,
  panelRef,
  initialFocusRef,
  onClose,
  canClose,
}: ModalFocusOptions) {
  const onCloseRef = useRef(onClose);
  const canCloseRef = useRef(canClose);
  onCloseRef.current = onClose;
  canCloseRef.current = canClose;

  useEffect(() => {
    if (!open) return;
    const previousFocus = document.activeElement as HTMLElement | null;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (canCloseRef.current && !canCloseRef.current()) return;
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const panel = panelRef.current;
      if (!panel) return;
      const items = focusables(panel);
      if (items.length === 0) return;
      const first = items[0]!;
      const last = items[items.length - 1]!;
      const active = document.activeElement;

      // Focus escaped the modal (e.g. browser chrome or a re-render): pull it
      // back to the first control rather than letting Tab wander the page.
      if (!panel.contains(active)) {
        event.preventDefault();
        first.focus();
        return;
      }
      if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    (initialFocusRef?.current ?? focusables(panelRef.current ?? document.body)[0])
      ?.focus();
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      previousFocus?.focus();
    };
  }, [open, panelRef, initialFocusRef]);
}
