import { fireEvent, render, screen } from "@testing-library/react";
import { useRef, useState } from "react";
import { describe, expect, it } from "vitest";
import { useModalFocus } from "./use-modal-focus";

function Modal({
  canClose = true,
  onBeforeOpen,
}: {
  canClose?: boolean;
  onBeforeOpen?: (element: HTMLElement) => void;
}) {
  const [open, setOpen] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);
  const firstRef = useRef<HTMLButtonElement>(null);
  useModalFocus({
    open,
    panelRef,
    initialFocusRef: firstRef,
    onClose: () => setOpen(false),
    canClose: () => canClose,
  });
  return (
    <div>
      <button type="button" onClick={(event) => {
        onBeforeOpen?.(event.currentTarget);
        setOpen(true);
      }}>
        打开
      </button>
      <button type="button">页面其他按钮</button>
      {open ? (
        <div ref={panelRef} role="dialog" aria-modal="true">
          <button ref={firstRef} type="button">
            第一个
          </button>
          <button type="button" disabled>
            禁用
          </button>
          <button type="button">最后一个</button>
        </div>
      ) : null}
    </div>
  );
}

describe("useModalFocus", () => {
  it("focuses the initial control and traps Tab at both ends", () => {
    render(<Modal />);
    fireEvent.click(screen.getByRole("button", { name: "打开" }));
    const first = screen.getByRole("button", { name: "第一个" });
    const last = screen.getByRole("button", { name: "最后一个" });
    expect(document.activeElement).toBe(first);

    last.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBe(first);

    first.focus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);

    // Disabled controls never participate in the trap.
    first.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).not.toBe(
      screen.getByRole("button", { name: "禁用" }),
    );
  });

  it("pulls focus back into the modal when it escapes", () => {
    render(<Modal />);
    fireEvent.click(screen.getByRole("button", { name: "打开" }));
    screen.getByRole("button", { name: "页面其他按钮" }).focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "第一个" }),
    );
  });

  it("closes on Escape and restores focus to the opener", () => {
    render(<Modal />);
    const opener = screen.getByRole("button", { name: "打开" });
    opener.focus();
    fireEvent.click(opener);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("ignores Escape while closing is disallowed", () => {
    render(<Modal canClose={false} />);
    fireEvent.click(screen.getByRole("button", { name: "打开" }));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("does not steal focus while closed", () => {
    render(<Modal />);
    const other = screen.getByRole("button", { name: "页面其他按钮" });
    other.focus();
    expect(document.activeElement).toBe(other);
  });
});
