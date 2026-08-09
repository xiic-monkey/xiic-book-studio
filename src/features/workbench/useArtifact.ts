import { useQuery } from "@tanstack/react-query";

import { api } from "../../api";

export const artifactQueryKey = (projectId: number, artifactId: number) => [
  "artifact",
  projectId,
  artifactId,
] as const;

export function useArtifact(projectId?: number | null, artifactId?: number | null) {
  return useQuery({
    queryKey: artifactQueryKey(projectId ?? 0, artifactId ?? 0),
    queryFn: () => api.getArtifact(projectId as number, artifactId as number),
    enabled: Boolean(projectId && artifactId),
    staleTime: 5 * 60_000,
  });
}
