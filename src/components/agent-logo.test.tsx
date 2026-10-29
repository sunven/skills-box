import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AgentLogo } from "./agent-logo";

describe("AgentLogo", () => {
  it("renders the bundled logo for a known agent", () => {
    const { container } = render(
      <AgentLogo agentId="claude-code" agentName="Claude Code" />,
    );
    const image = container.querySelector("img");
    expect(image).not.toBeNull();
    expect(image?.getAttribute("src")).toBeTruthy();
    expect(image).toHaveAttribute("alt", "");
  });

  it("falls back to initials when no logo is bundled", () => {
    const { container } = render(
      <AgentLogo agentId="unknown-agent" agentName="Unknown Agent" />,
    );
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toBe("UA");
  });
});
