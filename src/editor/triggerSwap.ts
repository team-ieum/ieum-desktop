import type { WfNode } from "../api";

export const isTrigger = (n: WfNode) => n.type?.toUpperCase() === "TRIGGER";
