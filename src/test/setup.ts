import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach } from "vitest";
import { resetFakeBackends } from "./fake-backend";

afterEach(() => {
  cleanup();
  resetFakeBackends();
  clearMocks();
  localStorage.clear();
});
