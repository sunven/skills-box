import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { pickProjectFolder } from "./lib/folder-picker";
import { createQueryClient } from "./lib/query-client";
import { updateReportQueryKey } from "./lib/update-view";
import { lastFullCheckQueryKey } from "./lib/update-check-lifecycle";
import {
  GLOBAL_SCOPE,
  type AppError,
  type InventorySnapshot,
  type ProjectEntry,
  type ProjectList,
  type ScopeId,
  type UpdateCheckResponse,
  type UpdateCheckState,
} from "./lib/types";
import { createFakeBackend, emptyUpdateReport } from "./test/fake-backend";
import { makeSkill, makeSnapshot } from "./test/inventory-fixtures";

vi.mock("./lib/folder-picker", () => ({ pickProjectFolder: vi.fn() }));

const ID = "11111111-1111-4111-8111-111111111111";
const SCOPE: ScopeId = `project:${ID}`;
const ROOT = "/tmp/project";
const INITIAL: ProjectEntry = {
  id: ID,
  root: ROOT,
  displayName: "project",
  addedAt: 1,
  lastOpenedAt: 1,
  checkRevision: "initial",
  available: true,
};

function projects(
  entry: ProjectEntry,
  lastActiveScope: ScopeId = SCOPE,
): ProjectList {
  return { projects: [entry], lastActiveScope, warnings: [] };
}

function state(revision: string, candidateCount = 0): UpdateCheckState {
  return {
    version: 1,
    checkRevision: revision,
    lastAttemptAt: Date.now(),
    lastSuccessAt: Date.now(),
    lastOutcome: "ok",
    failedSourceCount: 0,
    cachedSourceCount: 0,
    skippedSourceCount: 0,
    candidateCount,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function renderApp(client = createQueryClient()) {
  render(
    <QueryClientProvider client={client}>
      <App />
    </QueryClientProvider>,
  );
  return client;
}

async function confirmRelocate() {
  fireEvent.click(
    await screen.findByRole("button", { name: "重新定位项目 project" }),
  );
  const dialog = await screen.findByTestId("project-dialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "选择文件夹" }));
  await within(dialog).findByText("/tmp/moved");
  fireEvent.click(within(dialog).getByRole("button", { name: "重新定位" }));
  await waitFor(() =>
    expect(screen.queryByTestId("project-dialog")).not.toBeInTheDocument(),
  );
}

describe("Project Update check lifecycle in the app", () => {
  beforeEach(() => {
    vi.mocked(pickProjectFolder).mockReset().mockResolvedValue("/tmp/moved");
  });

  it.each(["response", "error"] as const)(
    "checks the relocated root before the old %s settles",
    async (completion) => {
      const old = deferred<UpdateCheckResponse>();
      const next = deferred<UpdateCheckResponse>();
      const inventory = deferred<InventorySnapshot>();
      const oldReport = emptyUpdateReport({
        checkedAtEpochMs: Date.now() - 1000,
      });
      const backend = createFakeBackend({
        data: {
          projects: projects(INITIAL),
          snapshots: {
            [SCOPE]: makeSnapshot({
              skills: [
                makeSkill({
                  id: `${SCOPE}/alpha`,
                  name: "alpha",
                  canonicalPath: `${ROOT}/.agents/skills/alpha`,
                }),
              ],
            }),
          },
          updateCheckStates: { [SCOPE]: state("initial", 2) },
        },
        commands: {
          check_updates: ({ checkRevision }) =>
            checkRevision === "initial" ? old.promise : next.promise,
          get_inventory: (args, fake) =>
            args.scope === SCOPE &&
            fake
              .callsOf("get_inventory")
              .filter((call) => call.payload.scope === SCOPE).length > 1
              ? inventory.promise
              : fake.dispatch("get_inventory", args),
        },
      }).install();
      const client = createQueryClient();
      client.setQueryData(lastFullCheckQueryKey(SCOPE), oldReport);
      renderApp(client);
      await screen.findByText("alpha");
      await waitFor(() =>
        expect(backend.callsOf("check_updates")).toHaveLength(1),
      );
      expect(
        within(
          screen.getByRole("button", { name: "切换到项目 project" }),
        ).getByTestId("scope-update-badge"),
      ).toHaveTextContent("2");

      // The path disappears while the initial network request is waiting.
      backend.update({
        projects: projects({
          ...INITIAL,
          available: false,
          unavailableReason: "目录不存在",
        }),
      });
      await act(async () =>
        client.invalidateQueries({ queryKey: ["projects"] }),
      );
      await confirmRelocate();
      await screen.findByRole("button", { name: "切换到项目 moved" });
      await waitFor(() =>
        expect(
          backend
            .callsOf("get_inventory")
            .filter((call) => call.payload.scope === SCOPE),
        ).toHaveLength(2),
      );
      expect(backend.callsOf("check_updates")).toHaveLength(1);
      expect(
        screen.queryByTestId("scope-update-badge"),
      ).not.toBeInTheDocument();
      expect(client.getQueryData(updateReportQueryKey(SCOPE))).toBeNull();
      expect(screen.getByTestId("project-last-check")).toHaveTextContent(
        "尚未检查",
      );

      await act(async () =>
        inventory.resolve(
          makeSnapshot({
            skills: [
              makeSkill({
                id: `${SCOPE}/beta`,
                name: "beta",
                canonicalPath: "/tmp/moved/.agents/skills/beta",
              }),
            ],
          }),
        ),
      );
      await waitFor(() =>
        expect(backend.callsOf("check_updates")).toHaveLength(2),
      );
      const revision =
        backend.callsOf("check_updates")[1]!.payload.checkRevision!;
      expect(revision).not.toBe("initial");
      await act(async () => {
        if (completion === "response")
          old.resolve({
            scope: SCOPE,
            checkRevision: "initial",
            report: oldReport,
            state: state("initial", 2),
          });
        else
          old.reject({
            code: "network.error",
            message: "旧路径检查失败",
            retryable: true,
          });
      });
      expect(screen.getByRole("button", { name: "检查更新" })).toHaveAttribute(
        "aria-busy",
        "true",
      );
      expect(screen.queryByText(/更新检查失败/)).not.toBeInTheDocument();
      expect(client.getQueryData(updateReportQueryKey(SCOPE))).toBeNull();
      await act(async () =>
        next.resolve({
          scope: SCOPE,
          checkRevision: revision,
          report: emptyUpdateReport({ checkedAtEpochMs: Date.now() }),
          state: state(revision),
        }),
      );
      await waitFor(() =>
        expect(
          screen.getByRole("button", { name: "检查更新" }),
        ).toHaveAttribute("aria-busy", "false"),
      );
      expect(screen.getByTestId("project-last-check")).toHaveTextContent(
        "上次检查",
      );
      expect(
        screen.queryByTestId("scope-update-badge"),
      ).not.toBeInTheDocument();
    },
  );

  it.each(["relocate", "forget"] as const)(
    "keeps a cleanup warning after the %s dialog closes",
    async (action) => {
      const warning: AppError = {
        code: "update.state_cleanup_failed",
        message: "项目变更已保存，但旧更新检查状态未能清理。",
        hint: "旧状态已失效，不会显示；可稍后重试更新检查。",
        retryable: false,
      };
      const backend = createFakeBackend({
        data: {
          projects: projects(
            action === "relocate" ? { ...INITIAL, available: false } : INITIAL,
            action === "relocate" ? GLOBAL_SCOPE : SCOPE,
          ),
        },
        commands: {
          relocate_project: async (args, fake) => ({
            ...(await fake.dispatch("relocate_project", args)),
            warnings: [warning],
          }),
          forget_project: async (args, fake) => ({
            ...(await fake.dispatch("forget_project", args)),
            warnings: [warning],
          }),
        },
      }).install();
      renderApp();
      if (action === "relocate") {
        await confirmRelocate();
        expect(
          await screen.findByRole("button", { name: "切换到项目 moved" }),
        ).toHaveAttribute("aria-pressed", "true");
        expect(backend.callsOf("relocate_project")).toHaveLength(1);
      } else {
        fireEvent.click(
          await screen.findByRole("button", { name: "遗忘项目 project" }),
        );
        const dialog = await screen.findByTestId("project-dialog");
        fireEvent.click(
          within(dialog).getByRole("button", { name: "遗忘项目" }),
        );
        await waitFor(() =>
          expect(
            screen.queryByTestId("project-dialog"),
          ).not.toBeInTheDocument(),
        );
        expect(
          screen.queryByRole("button", { name: "切换到项目 project" }),
        ).not.toBeInTheDocument();
        expect(backend.callsOf("forget_project")).toHaveLength(1);
      }
      expect(
        await screen.findByTestId("project-change-warning"),
      ).toHaveTextContent(warning.message);
      fireEvent.click(screen.getByRole("button", { name: "切换到全局" }));
      expect(screen.getByTestId("project-change-warning")).toHaveTextContent(
        warning.hint!,
      );
      fireEvent.click(screen.getByRole("button", { name: "关闭项目变更提示" }));
      expect(
        screen.queryByTestId("project-change-warning"),
      ).not.toBeInTheDocument();
    },
  );
});
