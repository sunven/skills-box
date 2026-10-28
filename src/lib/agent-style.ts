const AGENT_PALETTE = [
  "#c96145",
  "#2f3437",
  "#3f7ef7",
  "#6e40c9",
  "#0f766e",
  "#a64b0a",
  "#b45309",
  "#be123c",
];

export function agentColor(id: string): string {
  let hash = 0;
  for (const character of id) {
    hash = (hash * 31 + character.charCodeAt(0)) | 0;
  }
  return AGENT_PALETTE[Math.abs(hash) % AGENT_PALETTE.length]!;
}

export function agentInitials(name: string): string {
  const words = name.split(/\s+/).filter(Boolean);
  if (words.length >= 2) {
    return `${words[0]![0] ?? ""}${words[1]![0] ?? ""}`.toUpperCase();
  }
  return name.slice(0, 2).toUpperCase();
}
