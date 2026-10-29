import { Channel } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import { API_COMMANDS, getInventory, startInstall } from "../lib/api";
import type {
  AgentAttachment,
  InventorySnapshot,
  TaskEvent,
} from "../lib/types";
import {
  attachment,
  claudeAgent,
  makeSnapshot,
  makeSkill,
} from "./inventory-fixtures";
import { createFakeBackend } from "./fake-backend";

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
    });
  });

  it("updates the snapshot for native attach, detach and extra agents", () => {
    const backend = createFakeBackend({
      data: { snapshot: snapshotWith() },
    });

    const attached = backend.dispatch(API_COMMANDS.attachAgent, {
      skillId: "alpha",
      agentId: "claude-code",
    });
    expect(attached).toMatchObject({ agentId: "claude-code" });
    expect(backend.calls).toHaveLength(0);
    let snapshot = getSnapshot(backend);
    expect(snapshot.skills[0]!.attachments).toHaveLength(1);

    const detached = backend.dispatch(API_COMMANDS.detachAgent, {
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
            {},
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
  return backend.dispatch(API_COMMANDS.getInventory, {}) as InventorySnapshot;
}
