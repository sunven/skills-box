import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { CircleHelp } from "lucide-react";
import { Button } from "./button";

interface CommandHelpProps {
  label: string;
  children: ReactNode;
}

export function CommandHelp({ label, children }: CommandHelpProps) {
  const id = useId();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const pinned = useRef(false);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<{ top: number; left: number; height: number; side: "top" | "bottom" } | null>(null);

  const dismiss = () => { pinned.current = false; setOpen(false); };
  const show = () => {
    document.dispatchEvent(new CustomEvent("command-help-open", { detail: id }));
    setOpen(true);
  };
  const leave = () => {
    if (!pinned.current && document.activeElement !== buttonRef.current) dismiss();
  };

  useEffect(() => {
    const otherHelp = (event: Event) => {
      if ((event as CustomEvent<string>).detail !== id) dismiss();
    };
    document.addEventListener("command-help-open", otherHelp);
    return () => document.removeEventListener("command-help-open", otherHelp);
  }, [id]);

  useLayoutEffect(() => {
    if (!open || !buttonRef.current || !tooltipRef.current) return;
    const anchor = buttonRef.current.getBoundingClientRect();
    const tooltip = tooltipRef.current;
    const content = tooltip.firstElementChild as HTMLElement;
    const height = Math.min(content.scrollHeight + 26, 346);
    const below = Math.max(0, window.innerHeight - anchor.bottom - 22);
    const above = Math.max(0, anchor.top - 22);
    const side = height > below && above > below ? "top" : "bottom";
    const available = side === "top" ? above : below;
    const visibleHeight = Math.min(height, available);
    setPosition({
      top: side === "top" ? anchor.top - 6 - visibleHeight : anchor.bottom + 6,
      left: Math.max(16, Math.min(anchor.right - tooltip.offsetWidth, window.innerWidth - tooltip.offsetWidth - 16)),
      height: Math.max(0, available - 26),
      side,
    });
  }, [open, children]);

  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!buttonRef.current?.contains(target) && !tooltipRef.current?.contains(target)) dismiss();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      dismiss();
    };
    const scroll = (event: Event) => {
      if (!(event.target instanceof Node) || !tooltipRef.current?.contains(event.target)) dismiss();
    };
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", escape, true);
    document.addEventListener("scroll", scroll, true);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("keydown", escape, true);
      document.removeEventListener("scroll", scroll, true);
      window.removeEventListener("resize", dismiss);
    };
  }, [open]);

  return (
    <span className="inline-flex shrink-0" onMouseEnter={show} onMouseLeave={leave}>
      <Button
        ref={buttonRef}
        type="button"
        variant="ghost"
        size="icon"
        aria-label={label}
        aria-describedby={open ? id : undefined}
        onClick={() => { pinned.current = true; show(); }}
        onFocus={show}
        onBlur={(event) => {
          if (!tooltipRef.current?.contains(event.relatedTarget) &&
            (event.relatedTarget !== null || !tooltipRef.current?.matches(":hover"))) dismiss();
        }}
      >
        <CircleHelp aria-hidden="true" />
      </Button>
      {open ? createPortal(
        <div
          ref={tooltipRef}
          id={id}
          role="tooltip"
          className="command-help-tooltip"
          data-placement={position?.side}
          style={{ top: position?.top, left: position?.left, visibility: position ? "visible" : "hidden" }}
          onMouseEnter={show}
          onMouseLeave={leave}
        >
          <div className="command-help-content" style={{ maxHeight: position ? Math.min(320, position.height) : 320 }}>
            {children}
          </div>
        </div>, document.body,
      ) : null}
    </span>
  );
}
