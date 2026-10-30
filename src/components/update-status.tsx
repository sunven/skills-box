import { UPDATE_LABELS } from "../lib/inventory-view";
import { updateBadgeTitle } from "../lib/update-view";
import type { UpdateViewSkill } from "../lib/types";

/** The five-state update chip shared by the list row and the detail pane. */
export function UpdateStatus({
  skill,
  testId,
}: {
  skill: UpdateViewSkill;
  testId: string;
}) {
  const tone =
    skill.update === "update"
      ? "accent"
      : skill.update === "error" || skill.update === "path_missing"
        ? "negative"
        : "muted";
  return (
    <span
      data-testid={testId}
      className="status-chip"
      data-tone={tone}
      title={updateBadgeTitle(skill)}
    >
      {UPDATE_LABELS[skill.update]}
    </span>
  );
}
