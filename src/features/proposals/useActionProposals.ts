import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../../api";
import type { ActionProposal } from "../../types";

function proposalKey(projectId: number | null) {
  return ["action-proposals", projectId] as const;
}

export function useActionProposals(projectId: number | null) {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: proposalKey(projectId),
    enabled: projectId != null,
    queryFn: () => api.listActionProposals({ project_id: projectId!, status: "pending" }),
  });

  const merge = (incoming: ActionProposal[]) => {
    if (projectId == null || incoming.length === 0) return;
    queryClient.setQueryData<ActionProposal[]>(proposalKey(projectId), (current = []) => {
      const byId = new Map(current.map((proposal) => [proposal.id, proposal]));
      for (const proposal of incoming) byId.set(proposal.id, proposal);
      return [...byId.values()].filter((proposal) => proposal.status === "pending");
    });
  };

  const invalidate = async () => {
    if (projectId == null) return;
    await queryClient.invalidateQueries({ queryKey: proposalKey(projectId) });
  };

  return {
    proposals: query.data ?? [],
    error: query.error,
    merge,
    invalidate,
  };
}
