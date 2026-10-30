import { Channel } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import {
  API_COMMANDS,
  getInventory,
  listProjects,
  startInstall,
} from "../lib/api";
import type {
  AgentAttachment,
  InventorySnapshot,
  ScopeId,
  TaskEvent,
  UpdateSkillResult,
} from "../lib/types";
import { GLOBAL_SCOPE } from "../lib/types";
import {
  attachment,
  claudeAgent,
  makeSnapshot,
  makeSkill,
} from "./inventory-fixtures";
import { createFakeBackend, emptyUpdateReport } from "./fake-backend";

function snapshotWith(
  attachments: AgentAttachment[] = [],
): InventorySnapshot {
  return makeSnapshot({
    skills: [
      makeSkill({
        name: "alpha",
        source: "acme/skills",
        attachments,
      }),
    ],
    agents: [claudeAgent],
  });
}

describe("fake backend", () => {
  it("implements every command in the API gateway", () => {
    const backend = createFakeBackend();
    for (const command of Object.values(API_COMMANDS)) {
      expect(backend.supports(command), command).toBe(true);
    }
    expect(new Set(Object.values(API_COMMANDS)).size).toBe(
      Object.values(API_COMMANDS).length,
    );
  });

  it("records calls and returns data through the real api gateway", async () => {
    const backend = createFakeBackend({
      data: { snapshot: snapshotWith() },
    }).install();

    await expect(getInventory()).resolves.toMatchObject({
      skills: [{ name: "alpha" }],
    });
    expect(backend.count(API_COMMANDS.getInventory)).toBe(1);
    expect(backend.calls).toHaveLength(1);
    expect(backend.calls[0]).toMatchObject({
      command: API_COMMANDS.getInventory,
      payload: { scope: GLOBAL_SCOPE },
    });
  });

  it("exposes an empty project registry by default", async () => {
    createFakeBackend().install();
    await expect(listProjects()).resolves.toEqual({
      projects: [],
      lastActiveScope: GLOBAL_SCOPE,
      warnings: [],
    });
  });

  it("updates the snapshot for native attach, detach and extra agents", () => {
    const backend = createFakeBackend({
      data: { snapshot: snapshotWith() },
    });

    const attached = backend.dispatch(API_COMMANDS.attachAgent, {
      scope: GLOBAL_SCOPE,
      skillId: "alpha",
      agentId: "claude-code",
    });
    expect(attached).toMatchObject({ agentId: "claude-code" });
    expect(backend.calls).toHaveLength(0);
    let snapshot = getSnapshot(backend);
    expect(snapshot.skills[0]!.attachments).toHaveLength(1);

    const detached = backend.dispatch(API_COMMANDS.detachAgent, {
      scope: GLOBAL_SCOPE,
      skillId: "alpha",
      agentId: "claude-code",
    });
    expect(detached).toMatchObject({ unattached: true });
    snapshot = getSnapshot(backend);
    expect(snapshot.skills[0]!.attachments).toHaveLength(0);

    backend.dispatch(API_COMMANDS.addExtraAgent, {
      agent: {
        id: "acme",
        displayName: "Acme",
        skillsDir: "~/.acme/skills",
      },
    });
    expect(getSnapshot(backend).agents).toHaveLength(2);
    expect(
      backend.dispatch(API_COMMANDS.removeExtraAgent, { agentId: "acme" }),
    ).toMatchObject({ id: "acme", detached: [] });
    expect(getSnapshot(backend).agents).toHaveLength(1);
  });

  it("applies preview and install to the selected scope snapshot", () => {
    vi.useFakeTimers();
    try {
      const scope = "project:p1";
      const backend = createFakeBackend({
        data: {
          projects: {
            projects: [
              {
                id: "p1",
                root: "/tmp/proj",
                displayName: "proj",
                addedAt: 1,
                lastOpenedAt: 1,
                available: true,
              },
            ],
            lastActiveScope: scope,
            warnings: [],
          },
          snapshots: { [scope]: makeSnapshot({ agents: [claudeAgent] }) },
          previews: {
            [scope]: {
              source: "proj/skills",
              skills: [{ name: "beta", description: "Project beta" }],
              parseFailed: false,
              noSkills: false,
            },
          },
        },
        autoTasks: { delayMs: 10 },
      }).install();
      const channel = new Channel<TaskEvent>();
      const received: TaskEvent[] = [];
      channel.onmessage = (event) => received.push(event);

      backend.dispatch(API_COMMANDS.startPreview, {
        source: "proj/skills",
        scope,
        onEvent: channel,
      });
      vi.advanceTimersByTime(50);
      expect(received[received.length - 1]?.result?.preview?.source).toBe(
        "proj/skills",
      );

      backend.dispatch(API_COMMANDS.startInstall, {
        request: {
          source: "proj/skills",
          skills: ["beta"],
          agents: [],
        },
        scope,
        onEvent: new Channel<TaskEvent>(),
      });
      vi.advanceTimersByTime(50);

      const project = backend.dispatch(API_COMMANDS.getInventory, {
        scope,
      }) as InventorySnapshot;
      expect(project.skills).toMatchObject([
        {
          name: "beta",
          id: `${scope}/beta`,
          canonicalPath: "/tmp/proj/.agents/skills/beta",
        },
      ]);
      const global = backend.dispatch(API_COMMANDS.getInventory, {
        scope: GLOBAL_SCOPE,
      }) as InventorySnapshot;
      expect(global.skills).toHaveLength(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("applies update and uninstall to the selected scope snapshot only", () => {
    vi.useFakeTimers();
    try {
      const scope = "project:p1";
      const projectSnapshot = makeSnapshot({
        skills: [
          makeSkill({ id: `${scope}/alpha`, name: "alpha" }),
          makeSkill({ id: `${scope}/beta`, name: "beta" }),
        ],
      });
      const globalSnapshot = makeSnapshot({
        skills: [
          makeSkill({ id: "alpha", name: "alpha" }),
          makeSkill({ id: "beta", name: "beta" }),
        ],
      });
      const backend = createFakeBackend({
        data: {
          snapshot: globalSnapshot,
          snapshots: { [scope]: projectSnapshot },
        },
        autoTasks: { delayMs: 10 },
      }).install();

      const updateEvents: TaskEvent[] = [];
      const updateChannel = new Channel<TaskEvent>();
      updateChannel.onmessage = (event) => updateEvents.push(event);
      backend.dispatch(API_COMMANDS.startUpdate, {
        skillIds: ["alpha"],
        scope,
        onEvent: updateChannel,
      });
      vi.advanceTimersByTime(50);
      expect(updateEvents[updateEvents.length - 1]?.result).toMatchObject({
        action: "update",
        scope,
      });

      let project = backend.dispatch(API_COMMANDS.getInventory, {
        scope,
      }) as InventorySnapshot;
      const updated = project.skills.find((skill) => skill.name === "alpha")!;
      expect(updated.localModification).toBe("clean");
      expect(updated.updatedAt).not.toBeNull();
      const global = backend.dispatch(API_COMMANDS.getInventory, {
        scope: GLOBAL_SCOPE,
      }) as InventorySnapshot;
      expect(
        global.skills.find((skill) => skill.name === "alpha")!.updatedAt,
      ).toBeNull();

      const uninstallEvents: TaskEvent[] = [];
      const uninstallChannel = new Channel<TaskEvent>();
      uninstallChannel.onmessage = (event) => uninstallEvents.push(event);
      backend.dispatch(API_COMMANDS.startUninstall, {
        skillId: `${scope}/beta`,
        scope,
        onEvent: uninstallChannel,
      });
      vi.advanceTimersByTime(50);
      expect(uninstallEvents[uninstallEvents.length - 1]?.result).toMatchObject({
        action: "uninstall",
        scope,
      });
      project = backend.dispatch(API_COMMANDS.getInventory, {
        scope,
      }) as InventorySnapshot;
      expect(project.skills.map((skill) => skill.name)).toEqual(["alpha"]);
      const globalAfter = backend.dispatch(API_COMMANDS.getInventory, {
        scope: GLOBAL_SCOPE,
      }) as InventorySnapshot;
      expect(globalAfter.skills.map((skill) => skill.name)).toEqual([
        "alpha",
        "beta",
      ]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("attaches and detaches in the selected scope only", () => {
    const scope = "project:p1";
    const projectSnapshot = makeSnapshot({
      skills: [makeSkill({ id: `${scope}/alpha`, name: "alpha" })],
      agents: [claudeAgent],
    });
    const backend = createFakeBackend({
      data: {
        snapshot: snapshotWith(),
        snapshots: { [scope]: projectSnapshot },
      },
    });
    backend.dispatch(API_COMMANDS.attachAgent, {
      scope,
      skillId: `${scope}/alpha`,
      agentId: "claude-code",
    });
    let project = backend.dispatch(API_COMMANDS.getInventory, {
      scope,
    }) as InventorySnapshot;
    expect(project.skills[0]!.attachments).toHaveLength(1);
    const global = backend.dispatch(API_COMMANDS.getInventory, {
      scope: GLOBAL_SCOPE,
    }) as InventorySnapshot;
    expect(global.skills[0]!.attachments).toHaveLength(0);

    backend.dispatch(API_COMMANDS.detachAgent, {
      scope,
      skillId: `${scope}/alpha`,
      agentId: "claude-code",
    });
    project = backend.dispatch(API_COMMANDS.getInventory, {
      scope,
    }) as InventorySnapshot;
    expect(project.skills[0]!.attachments).toHaveLength(0);
  });

  it("lets overrides fail and fall through to the default handler", async () => {
    let attempts = 0;
    createFakeBackend({
      data: { snapshot: snapshotWith([attachment("claude-code")]) },
      commands: {
        get_inventory: (_args, current) => {
          attempts += 1;
          if (attempts === 1) {
            throw {
              code: "inventory.read_failed",
              message: "boom",
              retryable: true,
            };
          }
          return current.dispatch(
            API_COMMANDS.getInventory,
            { scope: GLOBAL_SCOPE },
          ) as InventorySnapshot;
        },
      },
    }).install();

    await expect(getInventory()).rejects.toMatchObject({ message: "boom" });
    await expect(getInventory()).resolves.toMatchObject({
      skills: [{ name: "alpha" }],
    });
  });

  it("owns task channels and event ordering", async () => {
    const backend = createFakeBackend({
      data: { snapshot: snapshotWith() },
    }).install();
    const received: TaskEvent[] = [];
    const taskId = await startInstall(
      {
        source: "acme/skills",
        skills: ["alpha"],
        agents: ["claude-code"],
      },
      (event) => received.push(event),
    );
    const task = backend.lastTask;
    expect(task).not.toBeNull();
    expect(task!.taskId).toBe(taskId);
    expect(taskId).toBe("task-1");

    task!.emit({ taskId: task!.taskId, phase: "preparing", message: "准备" });
    task!.emit({ taskId: task!.taskId, phase: "completed", message: "完成" });
    expect(received.map((event) => event.phase)).toEqual([
      "preparing",
      "completed",
    ]);
    expect(backend.tasks).toHaveLength(1);
    expect(backend.count(API_COMMANDS.startInstall)).toBe(1);
  });

  it("serves per-scope check reports and derives each scope's state", () => {
    const projectScope: ScopeId =
      "project:11111111-1111-4111-8111-111111111111";
    const candidate: UpdateSkillResult = {
      name: "alpha",
      source: "acme/skills",
      ref: "main",
      lockHash: "a".repeat(64),
      remoteHash: "b".repeat(64),
      state: "update",
      cached: false,
      lastSuccessAtEpochMs: 123,
      previousState: null,
      error: null,
      note: null,
    };
    const backend = createFakeBackend({
      data: {
        updateReports: {
          [projectScope]: emptyUpdateReport({
            skills: [candidate],
            checkedAtEpochMs: 123,
            lastSuccessAtEpochMs: 123,
            sources: [
              {
                source: "acme/skills",
                ref: "main",
                status: "ok",
                skillCount: 1,
                cached: false,
                truncated: false,
                lastSuccessAtEpochMs: 123,
                error: null,
                note: null,
              },
            ],
          }),
        },
      },
    });

    expect(
      backend.dispatch(API_COMMANDS.checkUpdates, {
        skillIds: null,
        scope: projectScope,
      }),
    ).toMatchObject({ skills: [{ name: "alpha", state: "update" }] });
    expect(
      backend.dispatch(API_COMMANDS.getUpdateCheckState, {
        scope: projectScope,
      }),
    ).toMatchObject({
      version: 1,
      lastAttemptAt: 123,
      lastOutcome: "ok",
      candidateCount: 1,
    });
    // Other scopes keep their own (empty) state.
    expect(
      backend.dispatch(API_COMMANDS.getUpdateCheckState, {
        scope: GLOBAL_SCOPE,
      }),
    ).toBeNull();
  });

  it("auto-completes preview tasks only when asked", () => {
    vi.useFakeTimers();
    try {
      const backend = createFakeBackend({
        data: { snapshot: snapshotWith() },
        autoTasks: { delayMs: 10 },
      }).install();
      const channel = new Channel<TaskEvent>();
      const received: TaskEvent[] = [];
      channel.onmessage = (event) => received.push(event);

      backend.dispatch(API_COMMANDS.startPreview, {
        source: "acme/skills",
        scope: GLOBAL_SCOPE,
        onEvent: channel,
      });
      vi.advanceTimersByTime(50);
      expect(received.map((event) => event.phase)).toEqual([
        "cloning",
        "completed",
      ]);
      expect(received[received.length - 1]?.result?.preview).toBeDefined();
    } finally {
      vi.useRealTimers();
    }
  });
});

function getSnapshot(backend: ReturnType<typeof createFakeBackend>): InventorySnapshot {
  return backend.dispatch(API_COMMANDS.getInventory, {
    scope: GLOBAL_SCOPE,
  }) as InventorySnapshot;
}
