import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
import { FileText, Link2, RefreshCw, Trash2 } from "lucide-react";
import { attachableAgents } from "../lib/detail-view";
import type { InventoryAgent, UpdateViewSkill } from "../lib/types";

export type SkillMenuAction = "content" | "check" | "attachments" | "uninstall";

export interface SkillMenuTarget {
  skillId: string;
  anchor: HTMLButtonElement;
}

export function SkillRowMenu({
  skill,
  target,
  agents,
  actionsDisabled,
  checkingUpdates,
  onAction,
  onClose,
}: {
  skill: UpdateViewSkill;
  target: SkillMenuTarget;
  agents: InventoryAgent[];
  actionsDisabled: boolean;
  checkingUpdates: boolean;
  onAction: (skill: UpdateViewSkill, action: SkillMenuAction) => void;
  onClose: (restoreFocus?: boolean) => void;
}) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{
    top: number;
    left: number;
  } | null>(null);
  const managed = skill.management === "managed";
  const canAttach = attachableAgents(agents, skill.attachments).length > 0;
  const canDetach = skill.attachments.some(
    (attachment) => attachment.detachable,
  );
  const scopeReason = actionsDisabled ? "当前作用域不可用或应用正在更新" : null;
  const managementReason = managed
    ? null
    : skill.management === "copy_only"
      ? "仅副本技能没有共享目录"
      : "未受管技能不支持此操作";
  const items = [
    {
      action: "content" as const,
      label: "查看内容",
      icon: FileText,
      reason: null,
    },
    {
      action: "check" as const,
      label: "检查更新",
      icon: RefreshCw,
      reason:
        scopeReason ??
        managementReason ??
        (checkingUpdates ? "正在检查更新，请稍候" : null),
    },
    {
      action: "attachments" as const,
      label: "管理 Agent 挂载",
      icon: Link2,
      reason:
        scopeReason ??
        managementReason ??
        (!skill.canonicalPath
          ? "技能目录不可用"
          : !canAttach && !canDetach
            ? "没有可挂载或可断开的 Agent"
            : null),
    },
    {
      action: "uninstall" as const,
      label: "卸载",
      icon: Trash2,
      reason: scopeReason,
    },
  ];

  const updatePosition = useCallback(() => {
    const menu = menuRef.current;
    if (!menu) return;
    const anchor = target.anchor.getBoundingClientRect();
    const list = target.anchor.closest(".list-scroll")?.getBoundingClientRect();
    if (
      !target.anchor.isConnected ||
      (list && (anchor.bottom < list.top || anchor.top > list.bottom))
    ) {
      onClose(false);
      return;
    }
    const rect = menu.getBoundingClientRect();
    const below = anchor.bottom + 6;
    setPosition({
      left: Math.max(
        8,
        Math.min(anchor.right - rect.width, window.innerWidth - rect.width - 8),
      ),
      top: Math.max(
        8,
        below + rect.height <= window.innerHeight - 8
          ? below
          : anchor.top - rect.height - 6,
      ),
    });
  }, [target, onClose]);

  useLayoutEffect(() => {
    updatePosition();
  }, [
    updatePosition,
    actionsDisabled,
    checkingUpdates,
    skill.management,
    skill.canonicalPath,
    canAttach,
    canDetach,
  ]);

  const positioned = position !== null;
  useLayoutEffect(() => {
    if (!positioned) return;
    menuRef.current
      ?.querySelector<HTMLElement>('[role="menuitem"]')
      ?.focus({ preventScroll: true });
  }, [target, positioned]);

  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (
        event.target instanceof Node &&
        !menuRef.current?.contains(event.target) &&
        !target.anchor.contains(event.target)
      )
        onClose(false);
    };
    const dismiss = () => onClose(false);
    const scroll = (event: Event) => {
      if (
        event.target instanceof Node &&
        menuRef.current?.contains(event.target)
      )
        return;
      updatePosition();
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("scroll", scroll, true);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("scroll", scroll, true);
      window.removeEventListener("resize", dismiss);
    };
  }, [target, onClose, updatePosition]);

  const handleKeys = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose(true);
      return;
    }
    if (event.key === "Tab") {
      onClose(true);
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const buttons = [
      ...(menuRef.current?.querySelectorAll<HTMLButtonElement>(
        '[role="menuitem"]',
      ) ?? []),
    ];
    const current = buttons.indexOf(
      document.activeElement as HTMLButtonElement,
    );
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? buttons.length - 1
          : (current + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) %
            buttons.length;
    buttons[next]?.focus();
  };

  return createPortal(
    <div
      ref={menuRef}
      id="skill-row-menu"
      role="menu"
      aria-label={`${skill.name} 的操作`}
      className="skill-row-menu"
      style={{
        top: position?.top ?? 0,
        left: position?.left ?? 0,
        visibility: position ? "visible" : "hidden",
      }}
      onKeyDown={handleKeys}
    >
      {items.map(({ action, label, icon: Icon, reason }) => (
        <button
          key={action}
          type="button"
          role="menuitem"
          tabIndex={-1}
          aria-disabled={Boolean(reason)}
          title={reason ?? label}
          className="skill-menu-item"
          data-danger={action === "uninstall" || undefined}
          onClick={() => {
            if (reason) return;
            onClose(true);
            onAction(skill, action);
          }}
        >
          <Icon aria-hidden="true" />
          <span>
            <span>{label}</span>
            {reason ? <small>{reason}</small> : null}
          </span>
        </button>
      ))}
    </div>,
    document.body,
  );
}
