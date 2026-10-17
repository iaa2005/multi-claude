import { invoke } from "@tauri-apps/api/core";

/** What `claude-science status` reported, folded for the UI. */
export interface ClaudeScienceStatus {
  installed: boolean;
  running: boolean;
  port: number | null;
  version: string | null;
  /** Set when the binary exists but its status could not be read. */
  error: string | null;
}

export const claudeScienceApi = {
  status: () => invoke<ClaudeScienceStatus>("get_claude_science_status"),
};
