import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CommandHelp } from "./command-help";

describe("command help", () => {
  it("opens outside a clipped container and updates its content while open", () => {
    const view = (content: string) => <div data-testid="container" style={{ overflow: "hidden" }}><CommandHelp label="查看命令">{content}</CommandHelp></div>;
    const rendered = render(view("正在准备执行命令…"));
    const button = screen.getByRole("button", { name: "查看命令" });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    fireEvent.click(button);
    expect(screen.getByRole("tooltip")).toHaveTextContent("正在准备执行命令…");
    expect(screen.getByTestId("container")).not.toContainElement(screen.getByRole("tooltip"));
    expect(button).toHaveAttribute("aria-describedby", screen.getByRole("tooltip").id);
    rendered.rerender(view("cd '/tmp/My project' && npx skills@latest add owner/repo -l"));
    expect(screen.getByRole("tooltip")).toHaveTextContent("cd '/tmp/My project' && npx skills@latest add owner/repo -l");
  });

  it("supports hover and keyboard focus and closes when they leave", () => {
    render(<><CommandHelp label="查看命令">命令</CommandHelp><button>下一项</button></>);
    const button = screen.getByRole("button", { name: "查看命令" });
    fireEvent.mouseEnter(button);
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    fireEvent.mouseLeave(button);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    act(() => button.focus());
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    vi.spyOn(screen.getByRole("tooltip"), "matches").mockReturnValue(true);
    act(() => screen.getByRole("button", { name: "下一项" }).focus());
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("keeps a clicked tooltip open for selection and scrolling until an outside click", () => {
    render(<CommandHelp label="查看命令">命令</CommandHelp>);
    const button = screen.getByRole("button", { name: "查看命令" });
    fireEvent.click(button);
    fireEvent.mouseLeave(button);
    const tooltip = screen.getByRole("tooltip");
    fireEvent.pointerDown(tooltip);
    fireEvent.scroll(tooltip.firstElementChild!);
    expect(screen.getByRole("tooltip")).toBeInTheDocument();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("dismisses before the dialog receives Escape", () => {
    const close = vi.fn();
    render(<div onKeyDown={close}><CommandHelp label="查看命令">命令</CommandHelp></div>);
    const button = screen.getByRole("button", { name: "查看命令" });
    fireEvent.click(button);
    fireEvent.keyDown(button, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    expect(close).not.toHaveBeenCalled();
    fireEvent.keyDown(button, { key: "Escape" });
    expect(close).toHaveBeenCalledOnce();
  });

  it("shows only the selected help and dismisses on resize or page scroll", () => {
    render(<><CommandHelp label="第一条">第一条命令</CommandHelp><CommandHelp label="第二条">第二条命令</CommandHelp></>);
    fireEvent.click(screen.getByRole("button", { name: "第一条" }));
    fireEvent.click(screen.getByRole("button", { name: "第二条" }));
    expect(screen.getAllByRole("tooltip")).toHaveLength(1);
    expect(screen.getByRole("tooltip")).toHaveTextContent("第二条命令");
    fireEvent(window, new Event("resize"));
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "第二条" }));
    fireEvent.scroll(document);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });
});
