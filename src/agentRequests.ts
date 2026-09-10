import type { AgentAction } from "./types";

// Retain the exact operation across transport errors and panel remounts.
export const agentRequests = {
  inFlight: false,
  pending: null as AgentAction | null,
  fresh(prompt: string, workspaceId: string): AgentAction {
    return Object.freeze({ action: "start", agentId: `desktop-${crypto.randomUUID()}`, requestKey: crypto.randomUUID(), prompt, workspaceId });
  },
  continuation(executionId: string, prompt: string): AgentAction {
    return Object.freeze({ action: "continue", executionId, requestKey: crypto.randomUUID(), prompt });
  },
  remember(request: AgentAction) { this.pending = request; },
  accepted(request: AgentAction) { if (this.pending === request) this.pending = null; },
};
