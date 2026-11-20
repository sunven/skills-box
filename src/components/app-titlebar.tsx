import { Settings } from "lucide-react";
import { Button } from "./ui/button";

interface AppTitlebarProps {
  settingsOpen: boolean;
  disabled: boolean;
  onOpenSettings: () => void;
}

export function AppTitlebar({ settingsOpen, disabled, onOpenSettings }: AppTitlebarProps) {
  return (
    <div className="app-titlebar" role="toolbar" aria-label="窗口标题栏">
      <div className="app-titlebar-drag" data-tauri-drag-region>
        <span data-tauri-drag-region>Skills Box</span>
      </div>
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
