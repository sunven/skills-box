import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { asAppError, fallbackError, openSkillFolder } from "../lib/api";
import {
  checkRunQueryKey,
  type CheckRunState,
} from "../lib/update-check-lifecycle";
import { updateReportQueryKey } from "../lib/update-view";
import type {
  AppError,
  ScopeId,
  UpdateCheckReport,
  UpdateViewSkill,
} from "../lib/types";

interface RowActionNotice {
  name: string;
  error: AppError;
}

/** The list is keyed by scope, so old completions cannot affect a new list. */
export function useSkillRowActions({
  scope,
  checkRevision,
  checkingUpdates,
  checkError,
  skills,
  onCheckUpdates,
}: {
  scope: ScopeId;
  checkRevision?: string;
  checkingUpdates: boolean;
  checkError: AppError | null;
  skills: UpdateViewSkill[];
  onCheckUpdates: (names: string[]) => boolean;
}) {
  const queryClient = useQueryClient();
  const mounted = useRef(false);
  const pendingFolders = useRef(new Set<string>());
  const [openingFolders, setOpeningFolders] = useState<ReadonlySet<string>>(
    new Set(),
  );
  const [checkingName, setCheckingName] = useState<string | null>(null);
  const [notice, setNotice] = useState<RowActionNotice | null>(null);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const openFolder = useCallback(
    async (skill: UpdateViewSkill) => {
      if (pendingFolders.current.has(skill.id)) return;
      pendingFolders.current.add(skill.id);
      setOpeningFolders(new Set(pendingFolders.current));
      setNotice(null);
      try {
        await openSkillFolder(skill.id, scope);
      } catch (cause) {
        if (mounted.current) {
          setNotice({
            name: skill.name,
            error: asAppError(cause) ?? fallbackError("无法打开技能目录。"),
          });
        }
      } finally {
        pendingFolders.current.delete(skill.id);
        if (mounted.current) setOpeningFolders(new Set(pendingFolders.current));
      }
    },
    [scope],
  );

  const checkSkill = useCallback(
    (skill: UpdateViewSkill) => {
      if (!onCheckUpdates([skill.name])) return;
      setNotice(null);
      setCheckingName(skill.name);
    },
    [onCheckUpdates],
  );

  useEffect(() => {
    if (checkingName === null) return;
    // Read the current slot: query notifications may lag the click that
    // started a check, or coalesce a fast check's pending and settled states.
    const run = queryClient.getQueryData<CheckRunState>(
      checkRunQueryKey(scope, checkRevision),
    );
    if (run?.isPending) return;
    const report = queryClient.getQueryData<UpdateCheckReport>(
      updateReportQueryKey(scope),
    );
    const error =
      run?.error ??
      report?.skills.find((skill) => skill.name === checkingName)?.error;
    if (error) setNotice({ name: checkingName, error });
    setCheckingName(null);
  }, [
    checkingName,
    checkingUpdates,
    checkError,
    skills,
    queryClient,
    scope,
    checkRevision,
  ]);

  const dismissNotice = useCallback(() => setNotice(null), []);
  return {
    openingFolders,
    checkingName,
    notice,
    openFolder,
    checkSkill,
    dismissNotice,
  };
}
