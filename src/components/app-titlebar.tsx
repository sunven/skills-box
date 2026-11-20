import { createContext, useContext, useLayoutEffect, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";
import { ArrowLeft, Settings } from "lucide-react";
import { Button } from "./ui/button";

interface BackAction {
  label: string;
  onBack: () => void;
}

const BackActionContext = createContext<BackAction | null>(null);
const SetBackActionContext = createContext<Dispatch<SetStateAction<BackAction | null>>>(() => {});

export function TitlebarNavigationProvider({ children }: { children: ReactNode }) {
  const [backAction, setBackAction] = useState<BackAction | null>(null);
  return (
    <SetBackActionContext value={setBackAction}>
      <BackActionContext value={backAction}>{children}</BackActionContext>
    </SetBackActionContext>
  );
}

export function TitlebarBack({ label, onBack, active = true }: BackAction & { active?: boolean }) {
  const setBackAction = useContext(SetBackActionContext);
  useLayoutEffect(() => {
    if (!active) return;
    const action = { label, onBack };
    setBackAction(action);
    return () => setBackAction((current) => current === action ? null : current);
  }, [active, label, onBack, setBackAction]);
  return null;
}

interface AppTitlebarProps {
  settingsOpen: boolean;
  disabled: boolean;
  onOpenSettings: () => void;
  onBack?: () => void;
}

export function AppTitlebar({ settingsOpen, disabled, onOpenSettings, onBack }: AppTitlebarProps) {
  const pageBack = useContext(BackActionContext);
  const backAction = pageBack ?? (onBack ? { label: "返回 skills", onBack } : null);
  return (
    <div className="app-titlebar" role="toolbar" aria-label="窗口标题栏">
      <span className="app-titlebar-name" data-tauri-drag-region>Skills Box</span>
      {backAction ? (
        <Button variant="ghost" size="sm" className="titlebar-back" disabled={disabled} onClick={backAction.onBack}>
          <ArrowLeft aria-hidden="true" />
          {backAction.label}
        </Button>
      ) : null}
      <div className="app-titlebar-drag" data-tauri-drag-region />
      <Button
        variant="ghost"
        size="icon"
        className="titlebar-settings"
        aria-label="设置"
        aria-pressed={settingsOpen}
        title="设置"
        disabled={disabled}
        onClick={onOpenSettings}
      >
        <Settings aria-hidden="true" />
      </Button>
    </div>
  );
}
