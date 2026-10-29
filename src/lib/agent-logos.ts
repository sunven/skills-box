const logoModules = import.meta.glob<string>(
  "../assets/agents/*.{svg,png}",
  { eager: true, query: "?url", import: "default" },
);

const logoById = new Map<string, string>();
for (const [path, url] of Object.entries(logoModules)) {
  const id = /\/([^/]+)\.(?:svg|png)$/.exec(path)?.[1];
  if (id) logoById.set(id, url);
}

export function agentLogoUrl(agentId: string): string | undefined {
  return logoById.get(agentId);
}
