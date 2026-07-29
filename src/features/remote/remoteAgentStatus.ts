import { getAgentStatusIndicatorClass } from "../../utils/statusUtils";

export const normalizedRemoteAgentStatus = (status: string): string =>
  status
    .trim()
    .toLowerCase()
    .replace(/\.+$/g, "")
    .replace(/\s+/g, "_");

export const isRemoteAgentOff = (status: string): boolean => {
  const normalized = normalizedRemoteAgentStatus(status);
  return normalized === "off" || normalized === "offline";
};

export const remoteStatusClassFor = (status: string): string => {
  return getAgentStatusIndicatorClass(status);
};
