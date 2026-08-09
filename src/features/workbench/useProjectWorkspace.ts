import { useQuery } from "@tanstack/react-query";

import { api } from "../../api";

export const projectWorkspaceQueryKey = (projectId: number) => [
  "project-workspace",
  projectId,
] as const;

export function useProjectWorkspace(projectId?: number | null) {
  return useQuery({
    queryKey: projectWorkspaceQueryKey(projectId ?? 0),
    queryFn: () => api.getProject(projectId as number),
    enabled: Boolean(projectId),
  });
}
