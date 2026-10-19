import type { Translator } from "./settingsTypes";

export type AgentMetaKey =
  | "antigravity"
  | "claude-code"
  | "codex"
  | "copilot"
  | "cursor"
  | "gemini"
  | "opencode"
  | "pi"
  | "workbuddy";

type AgentMetaCopy = {
  vendor: string;
  descriptionKey: Parameters<Translator>[0];
};

const AGENT_META: Record<AgentMetaKey, AgentMetaCopy> = {
  antigravity: {
    vendor: "Google",
    descriptionKey: "agentAntigravityDescription",
  },
  "claude-code": {
    vendor: "Anthropic",
    descriptionKey: "agentClaudeCodeDescription",
  },
  codex: {
    vendor: "OpenAI",
    descriptionKey: "agentCodexDescription",
  },
  copilot: {
    vendor: "GitHub",
    descriptionKey: "agentCopilotDescription",
  },
  cursor: {
    vendor: "Cursor",
    descriptionKey: "agentCursorDescription",
  },
  gemini: {
    vendor: "Google",
    descriptionKey: "agentGeminiDescription",
  },
  opencode: {
    vendor: "Open source",
    descriptionKey: "agentOpencodeDescription",
  },
  pi: {
    vendor: "Earendil Works",
    descriptionKey: "agentPiDescription",
  },
  workbuddy: {
    vendor: "Tencent",
    descriptionKey: "agentWorkbuddyDescription",
  },
};

export function agentMeta(agentId: string): AgentMetaCopy | null {
  return AGENT_META[agentId as AgentMetaKey] ?? null;
}
