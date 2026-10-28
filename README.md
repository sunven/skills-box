# Skills Box

A macOS-first desktop app for managing globally installed Agent Skills across
coding agents: inventory, install, update, detach, and uninstall.

## Development

```bash
pnpm install
pnpm tauri dev
```

## Checks

```bash
pnpm test        # Vitest + React Testing Library (mockIPC)
pnpm build       # tsc + vite production build
cd src-tauri && cargo test
```

## Context

- Product spec: `.scratch/global-skill-manager/spec.md`
- Domain glossary: `GLOSSARY.md`
- Architecture decision: `docs/adr/0001-cli-mutations-native-reads.md`
