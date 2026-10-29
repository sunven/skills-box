import { agentInitials } from "../lib/agent-style";
import { agentLogoUrl } from "../lib/agent-logos";
import { cn } from "../lib/utils";

interface AgentLogoProps {
  agentId: string;
  agentName: string;
  className?: string;
}

export function AgentLogo({ agentId, agentName, className }: AgentLogoProps) {
  const src = agentLogoUrl(agentId);

  if (!src) {
    return (
      <span
        aria-hidden="true"
        className={cn("agent-logo", "agent-logo-fallback", className)}
      >
        {agentInitials(agentName)}
      </span>
    );
  }

  return (
    <img
      src={src}
      alt=""
      aria-hidden="true"
      draggable={false}
      className={cn("agent-logo", className)}
    />
  );
}
