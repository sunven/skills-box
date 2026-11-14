import { fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProjectDialogs } from "./project-dialogs";
import { createQueryClient } from "../lib/query-client";
import { createFakeBackend } from "../test/fake-backend";
import type { ProjectEntry } from "../lib/types";

vi.mock("../lib/folder-picker", () => ({
  pickProjectFolder: async () => "/tmp/relocated",
}));

const project: ProjectEntry = {
  id: "one", root: "/tmp/one", displayName: "one", addedAt: 1,
  lastOpenedAt: 1, checkRevision: "initial", available: true,
};

describe("Project dialog failure refresh", () => {
  it.each(["forget", "relocate"] as const)(
    "refreshes the selected Project when %s rejects without a result Scope",
    async (mode) => {
      createFakeBackend({ commands: {
        forget_project: async () => { throw new Error("failed"); },
        relocate_project: async () => { throw new Error("failed"); },
      } }).install();
      const client = createQueryClient();
      for (const scope of ["global", "project:one", "project:two"]) {
        client.setQueryData(["inventory", scope], { scope });
        client.setQueryData(["updateReport", scope], { scope });
      }
      const onForgotten = vi.fn();
      const onRelocated = vi.fn();
      render(<QueryClientProvider client={client}>
        <ProjectDialogs state={{ mode, project }} onClose={vi.fn()} onAdded={vi.fn()}
          onForgotten={onForgotten} onRelocated={onRelocated} />
      </QueryClientProvider>);
      const dialog = within(screen.getByTestId("project-dialog"));
      if (mode === "relocate") {
        fireEvent.click(dialog.getByRole("button", { name: "选择文件夹" }));
        await dialog.findByText("/tmp/relocated");
      }
      fireEvent.click(dialog.getByRole("button", { name: mode === "forget" ? "遗忘项目" : "重新定位" }));
      await screen.findByTestId("project-dialog-error");
      for (const query of client.getQueryCache().getAll()) {
        const scope = query.queryKey[1];
        expect(query.state.data).toEqual({ scope });
        expect(query.state.isInvalidated).toBe(scope === "project:one");
      }
      expect(onForgotten).not.toHaveBeenCalled();
      expect(onRelocated).not.toHaveBeenCalled();
    },
  );
});
