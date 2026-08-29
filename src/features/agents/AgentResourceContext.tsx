import { createContext } from "react";
import type { AgentResourceController } from "./useAgentResourceController";

export const AgentResourceContext = createContext<AgentResourceController | null>(null);
