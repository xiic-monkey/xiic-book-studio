import {
  ArrowLeft,
  Check,
  AlertCircle,
  Bot,
  ChevronDown,
  Sparkles,
  Type,
  Database,
  Library,
  Palette,
  Loader2,
  KeyRound,
  Plus,
  RefreshCcw,
  Trash2,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Select } from "./Select";
import type {
  Agent,
  AiProvider,
  AiSettings,
  AgentToolDefinition,
  GenreAgentProfile,
  ModelInfo,
  ProviderCapabilities,
  SaveAiProvider,
  SaveWritingSkill,
  StorySearchStatus,
  ThinkingLevel,
  ToolProtocol,
  WritingSkill,
} from "../types";

type SettingsCategory = "ai" | "agents" | "skills" | "editor" | "data" | "appearance";

type ProviderConfig = AiProvider;

type AgentDraft = {
  name: string;
  role: string;
  system_prompt: string;
  provider_base_url: string;
  model: string;
  temperature: number;
  thinking_enabled: boolean;
  thinking_level: ThinkingLevel;
  uses_global_runtime_settings: boolean;
  enabled_tool_keys: string[];
  allowed_skill_keys: string[];
};

function agentDraftFromAgent(agent: Agent): AgentDraft {
  return {
    name: agent.name,
    role: agent.editable_role,
    system_prompt: agent.editable_system_prompt,
    provider_base_url: agent.provider_base_url,
    model: agent.model,
    temperature: agent.temperature,
    thinking_enabled: agent.thinking_enabled,
    thinking_level: agent.thinking_level,
    uses_global_runtime_settings: agent.uses_global_runtime_settings,
    enabled_tool_keys: [...agent.enabled_tool_keys],
    allowed_skill_keys: [...agent.allowed_skill_keys],
  };
}

function normalizeBaseUrl(baseUrl: string) {
  return baseUrl.trim();
}

function normalizeProvider(provider: Partial<ProviderConfig>, index: number): ProviderConfig {
  return {
    id: typeof provider.id === "number" ? provider.id : -(index + 1),
    label: provider.label?.trim() || `供应商 ${index + 1}`,
    base_url: normalizeBaseUrl(provider.base_url ?? ""),
    model: provider.model?.trim() || "",
    temperature: Number.isFinite(provider.temperature) ? Number(provider.temperature) : 0.75,
    thinking_enabled: Boolean(provider.thinking_enabled),
    thinking_level: provider.thinking_level ?? (provider.thinking_enabled ? "medium" : "off"),
    tool_protocol: provider.tool_protocol ?? "auto",
    has_api_key: Boolean(provider.has_api_key),
  };
}

function buildProviderFromSettings(settings: AiSettings): ProviderConfig {
  const baseUrl = normalizeBaseUrl(settings.base_url);
  return {
    id: -1,
    label: "当前配置",
    base_url: baseUrl,
    model: settings.model,
    temperature: settings.temperature,
    thinking_enabled: settings.thinking_enabled,
    thinking_level: settings.thinking_level,
    tool_protocol: "auto",
    has_api_key: settings.has_api_key,
  };
}

function normalizeAiSettings(settings: AiSettings): AiSettings {
  return {
    ...settings,
    base_url: normalizeBaseUrl(settings.base_url),
  };
}

function isCustomModel(model: string, availableModels: ModelInfo[]) {
  const value = model.trim();
  return value.length === 0 || !availableModels.some((item) => item.id === value);
}

function buildProviderState(settings: AiSettings, storedProviders: AiProvider[]) {
  const normalizedSettings = normalizeAiSettings(settings);
  const baseProviders = storedProviders.map((provider, index) => normalizeProvider(provider, index));
  const activeBaseUrl = normalizedSettings.base_url.trim();
  const matchedProvider = baseProviders.find((provider) => provider.base_url.trim() === activeBaseUrl);

  if (matchedProvider) {
    const providers = baseProviders.map((provider) =>
      provider.id === matchedProvider.id
          ? {
            ...provider,
            base_url: normalizedSettings.base_url,
            model: normalizedSettings.model,
            temperature: normalizedSettings.temperature,
            thinking_enabled: normalizedSettings.thinking_enabled,
            thinking_level: normalizedSettings.thinking_level,
            has_api_key: normalizedSettings.has_api_key,
          }
        : provider
    );
    return { providers, selectedProviderId: matchedProvider.id };
  }

  const activeProvider = buildProviderFromSettings(normalizedSettings);
  return {
    providers: [...baseProviders, activeProvider],
    selectedProviderId: activeProvider.id,
  };
}

const categories: { id: SettingsCategory; label: string; icon: React.ReactNode }[] = [
  { id: "ai", label: "服务配置", icon: <Sparkles size={16} /> },
  { id: "agents", label: "Agent 角色", icon: <Bot size={16} /> },
  { id: "skills", label: "技能库", icon: <Library size={16} /> },
  { id: "editor", label: "编辑器", icon: <Type size={16} /> },
  { id: "data", label: "数据管理", icon: <Database size={16} /> },
  { id: "appearance", label: "外观", icon: <Palette size={16} /> },
];

interface SettingsViewProps {
  settings: AiSettings;
  providers: AiProvider[];
  projectId?: number | null;
  storySearchStatus?: StorySearchStatus | null;
  apiKey: string;
  settingsCategory: SettingsCategory;
  onSettingsCategoryChange: (cat: SettingsCategory) => void;
  onBack: () => void;
  onSaveSettings: (settings: AiSettings, key: string) => Promise<boolean>;
  onSaveProvider: (input: SaveAiProvider) => Promise<AiProvider | null>;
  onDeleteProvider: (providerId: number) => Promise<boolean>;
  onGetProviderCapabilities: (providerBaseUrl: string) => Promise<ProviderCapabilities>;
  onSaveAgentSettings: (input: {
    agent_id: number;
    provider_base_url: string;
    model: string;
    name?: string | null;
    role?: string | null;
    system_prompt?: string | null;
    temperature?: number | null;
    thinking_enabled: boolean;
    thinking_level?: string | null;
    uses_global_runtime_settings?: boolean | null;
    enabled_tool_keys?: string[] | null;
    allowed_skill_keys?: string[] | null;
  }) => Promise<Agent | null>;
  onResetAgentPrompt: (agentId: number) => Promise<Agent | null>;
  onTestConnection: (settings: AiSettings, key: string) => Promise<void>;
  onRefreshModels: (input?: { base_url?: string | null; api_key?: string | null }) => Promise<ModelInfo[]>;
  onRefreshStorySearchStatus: (projectId?: number | null) => Promise<StorySearchStatus | null>;
  onRebuildStorySearch: () => Promise<void>;
  agents: Agent[];
  agentTools: AgentToolDefinition[];
  genreAgent?: GenreAgentProfile | null;
  writingSkills: WritingSkill[];
  onSaveWritingSkill: (input: SaveWritingSkill) => Promise<void>;
  onSaveCategory: (category: SettingsCategory) => Promise<void>;
  busy: string | null;
  notice: string | null;
  error: string | null;
}

export function SettingsView({
  settings,
  providers: savedProviders,
  projectId,
  storySearchStatus,
  apiKey,
  settingsCategory,
  onSettingsCategoryChange,
  onBack,
  onSaveSettings,
  onSaveProvider,
  onDeleteProvider,
  onGetProviderCapabilities,
  onSaveAgentSettings,
  onResetAgentPrompt,
  onTestConnection,
  onRefreshModels,
  onRefreshStorySearchStatus,
  onRebuildStorySearch,
  agents,
  agentTools,
  genreAgent,
  writingSkills,
  onSaveWritingSkill,
  busy,
  notice,
  error,
}: SettingsViewProps) {
  const initialProviderState = buildProviderState(settings, savedProviders);
  const [aiSettings, setAiSettings] = useState<AiSettings>(settings);
  const [localApiKey, setLocalApiKey] = useState(apiKey);
  const [providers, setProviders] = useState<ProviderConfig[]>(initialProviderState.providers);
  const [selectedProviderId, setSelectedProviderId] = useState(initialProviderState.selectedProviderId);
  const providerCatalogInitialized = useRef(savedProviders.length > 0);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [loadingModels, setLoadingModels] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);
  const [providerCapabilities, setProviderCapabilities] = useState<ProviderCapabilities | null>(null);
  const [providerCapabilitiesError, setProviderCapabilitiesError] = useState<string | null>(null);
  const [loadingProviderCapabilities, setLoadingProviderCapabilities] = useState(false);
  const [agentDrafts, setAgentDrafts] = useState<Record<number, AgentDraft>>({});
  const [agentModels, setAgentModels] = useState<Record<number, ModelInfo[]>>({});
  const [loadingAgentModels, setLoadingAgentModels] = useState<number | null>(null);
  const [agentModelError, setAgentModelError] = useState<string | null>(null);
  const [selectedAgentId, setSelectedAgentId] = useState<number | null>(null);
  const [selectedSkillKey, setSelectedSkillKey] = useState("");
  const [skillDraft, setSkillDraft] = useState<SaveWritingSkill | null>(null);
  const [localStorySearchStatus, setLocalStorySearchStatus] = useState<StorySearchStatus | null>(
    storySearchStatus ?? null
  );
  const isTestingConnection = busy === "测试连接";
  const isConnectionSuccessNotice = notice?.startsWith("连接成功：") ?? false;
  const [testConnectionToast, setTestConnectionToast] = useState<string | null>(null);
  const [errorToast, setErrorToast] = useState<string | null>(null);

  useEffect(() => {
    if (!notice || !isConnectionSuccessNotice) {
      setTestConnectionToast(null);
      return;
    }

    setTestConnectionToast(notice);
    const timer = window.setTimeout(() => setTestConnectionToast(null), 2500);
    return () => window.clearTimeout(timer);
  }, [isConnectionSuccessNotice, notice]);

  useEffect(() => {
    if (!error) {
      setErrorToast(null);
      return;
    }

    setErrorToast(error);
    const timer = window.setTimeout(() => setErrorToast(null), 2500);
    return () => window.clearTimeout(timer);
  }, [error]);

  useEffect(() => {
    const nextProviderState = buildProviderState(settings, savedProviders);
    setAiSettings(normalizeAiSettings(settings));
    setLocalApiKey(apiKey);
    setProviders(nextProviderState.providers);
    setSelectedProviderId(nextProviderState.selectedProviderId);
    setModels([]);
    setModelError(null);
  }, [settings, apiKey]);

  useEffect(() => {
    if (providerCatalogInitialized.current || savedProviders.length === 0) return;
    providerCatalogInitialized.current = true;
    const nextProviderState = buildProviderState(settings, savedProviders);
    setProviders(nextProviderState.providers);
    setSelectedProviderId(nextProviderState.selectedProviderId);
  }, [savedProviders, settings]);

  useEffect(() => {
    setLocalStorySearchStatus(storySearchStatus ?? null);
  }, [storySearchStatus]);

  useEffect(() => {
    setAgentDrafts((current) => {
      const next: typeof current = {};
      for (const agent of agents) {
        next[agent.id] = current[agent.id] ?? agentDraftFromAgent(agent);
      }
      return next;
    });
  }, [agents]);

  useEffect(() => {
    setSelectedAgentId((current) =>
      agents.some((agent) => agent.id === current) ? current : agents[0]?.id ?? null
    );
  }, [agents]);

  useEffect(() => {
    if (!projectId) {
      setLocalStorySearchStatus(null);
      return;
    }
    void onRefreshStorySearchStatus(projectId)
      .then((status) => setLocalStorySearchStatus(status))
      .catch(() => setLocalStorySearchStatus(null));
  }, [projectId]);

  useEffect(() => {
    if (writingSkills.length === 0) {
      setSelectedSkillKey("");
      setSkillDraft(null);
      return;
    }
    const selected =
      writingSkills.find((skill) => skill.skill_key === selectedSkillKey) ?? writingSkills[0];
    setSelectedSkillKey(selected.skill_key);
    setSkillDraft({
      id: selected.id,
      skill_key: selected.skill_key,
      name: selected.name,
      category: selected.category,
      description: selected.description,
      content: selected.content,
      enabled: selected.enabled,
    });
  }, [writingSkills, selectedSkillKey]);

  const currentProvider =
    providers.find((provider) => provider.id === selectedProviderId) ?? providers[0] ?? null;
  const customGlobalModel = isCustomModel(aiSettings.model, models);

  useEffect(() => {
    const providerBaseUrl = currentProvider?.base_url.trim();
    if (!providerBaseUrl) {
      setProviderCapabilities(null);
      setProviderCapabilitiesError(null);
      return;
    }
    let cancelled = false;
    setLoadingProviderCapabilities(true);
    setProviderCapabilitiesError(null);
    void onGetProviderCapabilities(providerBaseUrl)
      .then((capabilities) => {
        if (!cancelled) setProviderCapabilities(capabilities);
      })
      .catch((err) => {
        if (!cancelled) {
          setProviderCapabilities(null);
          setProviderCapabilitiesError(String(err));
        }
      })
      .finally(() => {
        if (!cancelled) setLoadingProviderCapabilities(false);
      });
    return () => {
      cancelled = true;
    };
  }, [currentProvider?.base_url, onGetProviderCapabilities]);

  const handleSaveAi = async () => {
    if (!currentProvider) return;
    const savedProvider = await onSaveProvider({
      id: currentProvider.id > 0 ? currentProvider.id : null,
      label: currentProvider.label,
      base_url: aiSettings.base_url,
      model: aiSettings.model,
      temperature: aiSettings.temperature,
      thinking_enabled: aiSettings.thinking_enabled,
      thinking_level: aiSettings.thinking_level,
      tool_protocol: currentProvider.tool_protocol,
    });
    if (!savedProvider) return;

    setProviders((current) =>
      current.map((provider) =>
        provider.id === currentProvider.id ? savedProvider : provider
      )
    );
    setSelectedProviderId(savedProvider.id);
    const succeeded = await onSaveSettings(
      {
        ...aiSettings,
        base_url: savedProvider.base_url,
        model: savedProvider.model,
        temperature: savedProvider.temperature,
        thinking_enabled: savedProvider.thinking_enabled,
        thinking_level: savedProvider.thinking_level,
        has_api_key: savedProvider.has_api_key,
      },
      localApiKey
    );
    if (succeeded) setLocalApiKey("");
  };

  const handleTestConnection = async () => {
    await onTestConnection(aiSettings, localApiKey);
  };

  const handleRefreshModels = async () => {
    setLoadingModels(true);
    setModelError(null);
    try {
      const result = await onRefreshModels({
        base_url: aiSettings.base_url,
        api_key: localApiKey,
      });
      setModels(result);
    } catch (err) {
      setModelError(String(err));
      setModels([]);
    } finally {
      setLoadingModels(false);
    }
  };

  const handleSaveSkill = async () => {
    if (!skillDraft) return;
    await onSaveWritingSkill(skillDraft);
  };

  const updateAgentDraft = (agent: Agent, patch: Partial<AgentDraft>) => {
    setAgentDrafts((current) => ({
      ...current,
      [agent.id]: {
        name: current[agent.id]?.name ?? agent.name,
        role: current[agent.id]?.role ?? agent.editable_role,
        system_prompt: current[agent.id]?.system_prompt ?? agent.editable_system_prompt,
        provider_base_url: current[agent.id]?.provider_base_url ?? agent.provider_base_url,
        model: current[agent.id]?.model ?? agent.model,
        temperature: current[agent.id]?.temperature ?? agent.temperature,
        thinking_enabled: current[agent.id]?.thinking_enabled ?? agent.thinking_enabled,
        thinking_level: current[agent.id]?.thinking_level ?? agent.thinking_level,
        uses_global_runtime_settings:
          current[agent.id]?.uses_global_runtime_settings ?? agent.uses_global_runtime_settings,
        enabled_tool_keys: current[agent.id]?.enabled_tool_keys ?? [...agent.enabled_tool_keys],
        allowed_skill_keys: current[agent.id]?.allowed_skill_keys ?? [...agent.allowed_skill_keys],
        ...patch,
      },
    }));
  };

  const saveAgentDraft = async (agent: Agent) => {
    const draft = agentDrafts[agent.id] ?? agentDraftFromAgent(agent);
    const saved = await onSaveAgentSettings({ agent_id: agent.id, ...draft });
    if (saved) {
      setAgentDrafts((current) => ({ ...current, [agent.id]: agentDraftFromAgent(saved) }));
    }
  };

  const resetAgentPrompt = async (agent: Agent) => {
    if (!window.confirm(`恢复“${agent.name}”的 V2 默认系统提示词？当前自定义内容将被替换。`)) return;
    const saved = await onResetAgentPrompt(agent.id);
    if (saved) {
      setAgentDrafts((current) => ({ ...current, [agent.id]: agentDraftFromAgent(saved) }));
    }
  };

  const refreshAgentModels = async (agent: Agent) => {
    const draft = agentDrafts[agent.id] ?? agent;
    setLoadingAgentModels(agent.id);
    setAgentModelError(null);
    try {
      const result = await onRefreshModels({ base_url: draft.provider_base_url });
      setAgentModels((current) => ({ ...current, [agent.id]: result }));
    } catch (err) {
      setAgentModelError(`${agent.name}：${String(err)}`);
    } finally {
      setLoadingAgentModels(null);
    }
  };

  const handleRebuildStorySearch = async () => {
    await onRebuildStorySearch();
    const status = await onRefreshStorySearchStatus(projectId);
    setLocalStorySearchStatus(status);
  };

  const updateCurrentProvider = (patch: Partial<ProviderConfig>) => {
    setProviders((current) =>
      current.map((provider) =>
        provider.id === selectedProviderId
          // Keep transient empty values while the user is editing. Validation and
          // fallback labels belong at save/load boundaries, not on every keystroke.
          ? { ...provider, ...patch }
          : provider
      )
    );
  };

  const selectProvider = (providerId: number) => {
    const provider = providers.find((item) => item.id === providerId);
    if (!provider) return;
    setSelectedProviderId(providerId);
    setAiSettings((current) => ({
      ...current,
      base_url: provider.base_url,
      model: provider.model,
      temperature: provider.temperature,
      thinking_enabled: provider.thinking_enabled,
      thinking_level: provider.thinking_level,
      has_api_key: provider.has_api_key,
    }));
    setLocalApiKey("");
    setModels([]);
    setModelError(null);
  };

  const addProvider = () => {
    const nextIndex = providers.length + 1;
    const provider: ProviderConfig = {
      id: -Date.now(),
      label: `自定义供应商 ${nextIndex}`,
      base_url: "",
      model: "",
      temperature: aiSettings.temperature,
      thinking_enabled: aiSettings.thinking_enabled,
      thinking_level: aiSettings.thinking_level,
      tool_protocol: "auto",
      has_api_key: false,
    };
    setProviders((current) => [...current, provider]);
    setSelectedProviderId(provider.id);
    setAiSettings((current) => ({
      ...current,
      base_url: provider.base_url,
      model: provider.model,
      temperature: provider.temperature,
      thinking_enabled: provider.thinking_enabled,
      thinking_level: provider.thinking_level,
      has_api_key: false,
    }));
    setLocalApiKey("");
    setModels([]);
    setModelError(null);
  };

  const removeProvider = async (providerId: number) => {
    if (providers.length <= 1) return;
    if (providerId > 0 && !(await onDeleteProvider(providerId))) return;
    const nextProviders = providers.filter((provider) => provider.id !== providerId);
    setProviders(nextProviders);
    if (selectedProviderId !== providerId) return;
    const fallbackProvider = nextProviders[0];
    if (!fallbackProvider) return;
    setSelectedProviderId(fallbackProvider.id);
    setAiSettings((current) => ({
      ...current,
      base_url: fallbackProvider.base_url,
      model: fallbackProvider.model,
      temperature: fallbackProvider.temperature,
      thinking_enabled: fallbackProvider.thinking_enabled,
      thinking_level: fallbackProvider.thinking_level,
      has_api_key: fallbackProvider.has_api_key,
    }));
    setLocalApiKey("");
    setModels([]);
    setModelError(null);
  };

  const selectedAgent = agents.find((agent) => agent.id === selectedAgentId) ?? agents[0] ?? null;
  const selectedAgentDraft = selectedAgent
    ? agentDrafts[selectedAgent.id] ?? agentDraftFromAgent(selectedAgent)
    : null;
  const selectedAgentProviderOptions = selectedAgent && selectedAgentDraft
    ? (providers.some((provider) => provider.base_url.trim() === selectedAgentDraft.provider_base_url.trim())
        ? providers
        : [
            ...providers,
            {
              id: -selectedAgent.id,
              label: selectedAgentDraft.provider_base_url || "当前 Agent 供应商",
              base_url: selectedAgentDraft.provider_base_url,
              model: selectedAgentDraft.model,
              temperature: selectedAgent.temperature,
              thinking_enabled: selectedAgentDraft.thinking_enabled,
              thinking_level: selectedAgentDraft.thinking_level,
              tool_protocol: "auto" as ToolProtocol,
              has_api_key: false,
            },
          ])
    : [];

  const renderCategoryContent = () => {
    switch (settingsCategory) {
      case "ai":
        return (
          <div className="settings-content">
            <div className="settings-section">
              <h3>服务配置</h3>
              <div className="form-field">
                <label>Provider</label>
                <div className="provider-list">
                  {providers.map((provider) => (
                    <div
                      key={provider.id}
                      className={selectedProviderId === provider.id ? "provider-pill active" : "provider-pill"}
                    >
                      <button type="button" className="provider-pill-button" onClick={() => selectProvider(provider.id)}>
                        {provider.label}
                      </button>
                      {providers.length > 1 && (
                        <button
                          type="button"
                          className="provider-pill-remove"
                          onClick={() => void removeProvider(provider.id)}
                          aria-label={`删除 ${provider.label}`}
                        >
                          <Trash2 size={12} />
                        </button>
                      )}
                    </div>
                  ))}
                  <button type="button" className="provider-add-button" onClick={addProvider}>
                    <span className="provider-add-label">新增供应商</span>
                    <span className="provider-add-icon" aria-hidden="true">
                      <Plus size={14} />
                    </span>
                  </button>
                </div>
              </div>
              <div className="form-field">
                <label htmlFor="provider_name">供应商名称</label>
                <input
                  id="provider_name"
                  type="text"
                  value={currentProvider?.label ?? ""}
                  onChange={(e) => updateCurrentProvider({ label: e.target.value })}
                  placeholder="服务名称"
                />
              </div>
              <div className="form-field">
                <label htmlFor="base_url">API Base URL</label>
                <input
                  id="base_url"
                  type="text"
                  value={aiSettings.base_url}
                  onChange={(e) => {
                    const value = e.target.value;
                    setAiSettings({ ...aiSettings, base_url: value });
                    updateCurrentProvider({ base_url: value });
                  }}
                  placeholder="https://api.example.com/v1"
                />
              </div>
              <div className="form-field">
                <label htmlFor="model">模型名称</label>
                <div className="model-picker">
                  <div className="model-picker-row">
                    <Select
                      id="model"
                      value={customGlobalModel ? "" : aiSettings.model}
                      onChange={(value) => {
                        setAiSettings({ ...aiSettings, model: value });
                        updateCurrentProvider({ model: value });
                      }}
                      options={[
                        { value: "", label: "自定义模型" },
                        ...models.map((model) => ({
                          value: model.id,
                          label: `${model.id}${model.owned_by ? ` · ${model.owned_by}` : ""}`,
                        })),
                      ]}
                    />
                    <button type="button" onClick={handleRefreshModels} disabled={loadingModels || Boolean(busy)}>
                      {loadingModels ? <Loader2 size={14} className="spin" /> : <ChevronDown size={14} />}
                      刷新
                    </button>
                  </div>
                  {customGlobalModel && (
                    <input
                      type="text"
                      value={aiSettings.model}
                      onChange={(e) => {
                        const value = e.target.value;
                        setAiSettings({ ...aiSettings, model: value });
                        updateCurrentProvider({ model: value });
                      }}
                      placeholder="模型名称"
                    />
                  )}
                  <div className="settings-hint-row">
                    {loadingModels && <span>正在拉取模型列表…</span>}
                    {!loadingModels && models.length > 0 && <span>已获取 {models.length} 个模型</span>}
                    {modelError && <span className="settings-error">{modelError}</span>}
                  </div>
                </div>
              </div>
              <div className="form-field">
                <label htmlFor="temperature">Temperature</label>
                <input
                  id="temperature"
                  type="number"
                  step="0.05"
                  min="0"
                  max="2"
                  value={aiSettings.temperature}
                  onChange={(e) => {
                    const value = Number(e.target.value);
                    setAiSettings({ ...aiSettings, temperature: value });
                    updateCurrentProvider({ temperature: value });
                  }}
                />
              </div>
              <div className="form-field">
                <label htmlFor="thinking_enabled">思考模式</label>
                <label className="checkbox-row">
                  <input
                    id="thinking_enabled"
                    type="checkbox"
                    checked={aiSettings.thinking_enabled}
                    onChange={(e) => {
                      const value = e.target.checked;
                      const thinking_level = value
                        ? (aiSettings.thinking_level === "off" ? "medium" : aiSettings.thinking_level)
                        : "off";
                      setAiSettings({ ...aiSettings, thinking_enabled: value, thinking_level });
                      updateCurrentProvider({ thinking_enabled: value, thinking_level });
                    }}
                  />
                  <span>启用思考</span>
                </label>
                <div className="form-field">
                  <label htmlFor="thinking_level">思考强度</label>
                  <Select
                    id="thinking_level"
                    value={aiSettings.thinking_level}
                    disabled={!aiSettings.thinking_enabled}
                    onChange={(value) => {
                      const thinking_level = value as AiSettings["thinking_level"];
                      setAiSettings({
                        ...aiSettings,
                        thinking_level,
                        thinking_enabled: thinking_level !== "off",
                      });
                      updateCurrentProvider({
                        thinking_level,
                        thinking_enabled: thinking_level !== "off",
                      });
                    }}
                    options={[
                      { value: "off", label: "关闭" },
                      { value: "low", label: "低" },
                      { value: "medium", label: "中" },
                      { value: "high", label: "高" },
                    ]}
                  />
                </div>
              </div>
              <div className="form-field">
                <label htmlFor="tool_protocol">工具调用协议</label>
                <Select
                  id="tool_protocol"
                  value={currentProvider?.tool_protocol ?? "auto"}
                  onChange={(value) => updateCurrentProvider({ tool_protocol: value as ToolProtocol })}
                  options={[
                    { value: "auto", label: "自动" },
                    { value: "native", label: "仅原生 tool calling" },
                    { value: "structured", label: "仅结构化 JSON 计划" },
                  ]}
                />
                <div className="settings-hint-row">
                  {loadingProviderCapabilities && <span>正在读取运行时能力状态…</span>}
                  {!loadingProviderCapabilities && providerCapabilities && (
                    <span>
                      实际探测：{providerCapabilities.detected_protocol === "native"
                        ? "原生 tool calling"
                        : providerCapabilities.detected_protocol === "structured"
                          ? "结构化 JSON"
                          : "尚未探测"}
                      {providerCapabilities.updated_at ? ` · ${providerCapabilities.updated_at}` : ""}
                    </span>
                  )}
                  {providerCapabilities?.last_error && (
                    <span className="settings-error">最近能力探测：{providerCapabilities.last_error}</span>
                  )}
                  {providerCapabilitiesError && (
                    <span className="settings-error">能力状态读取失败：{providerCapabilitiesError}</span>
                  )}
                </div>
              </div>
              <div className="form-field">
                <label htmlFor="api_key">API Key</label>
                <input
                  id="api_key"
                  type="password"
                  value={localApiKey}
                  placeholder="新 Key（留空保留）"
                  onChange={(e) => setLocalApiKey(e.target.value)}
                />
              </div>
              <div className="button-row">
                <button onClick={handleSaveAi} disabled={Boolean(busy)} className="btn-primary">
                  {busy === "保存设置" ? <Loader2 size={14} className="spin" /> : <><KeyRound size={14} /> 保存</>}
                </button>
                <button onClick={handleTestConnection} disabled={Boolean(busy)}>
                  {isTestingConnection ? <><Loader2 size={14} className="spin" /> 测试中…</> : <><RefreshCcw size={14} /> 测试连接</>}
                </button>
              </div>
            </div>
          </div>
        );

      case "agents":
        return (
          <div className="settings-content wide">
            {genreAgent && (
              <section className="agent-prompt-card agent-profile-card">
                <header>
                  <div>
                    <strong>{genreAgent.name}</strong>
                    <span>{genreAgent.agent_key}</span>
                  </div>
                  <small>当前项目专属 Agent</small>
                </header>
                <p>{genreAgent.role}</p>
                <label>主类型 Skill</label>
                <p>{genreAgent.primary_skill_key}</p>
                <label>Skill 白名单</label>
                <p>{genreAgent.allowed_skill_keys.join(" · ")}</p>
              </section>
            )}
            {agentModelError && <p className="settings-error">{agentModelError}</p>}
            <div className="agent-settings-layout">
              <nav className="agent-list" aria-label="Agent 列表">
                {agents.map((agent) => {
                  const draft = agentDrafts[agent.id] ?? agentDraftFromAgent(agent);
                  const selected = agent.id === selectedAgent?.id;
                  return (
                    <button
                      key={agent.id}
                      type="button"
                      className={selected ? "agent-list-item active" : "agent-list-item"}
                      onClick={() => setSelectedAgentId(agent.id)}
                      aria-current={selected ? "true" : undefined}
                    >
                      <span className="agent-list-item-title">{draft.name || agent.name}</span>
                      <span className="agent-list-item-meta">{agent.stage} · {draft.uses_global_runtime_settings ? "继承全局" : draft.model || "未设模型"}</span>
                    </button>
                  );
                })}
              </nav>
              <div className="agent-editor-pane">
              {selectedAgent && selectedAgentDraft && (() => {
                const agent = selectedAgent;
                const draft = selectedAgentDraft;
                const agentProviderOptions = selectedAgentProviderOptions;
                const availableModels = agentModels[agent.id] ?? [];
                const customAgentModel = isCustomModel(draft.model, availableModels);
                return (
                  <section className="agent-prompt-card" key={agent.id}>
                    <header>
                      <div>
                        <strong>{draft.name || agent.name}</strong>
                        <span>{agent.stage}</span>
                      </div>
                      <small>Temperature {draft.temperature.toFixed(2)}</small>
                    </header>
                    <div className="agent-edit-grid">
                      <div className="form-field">
                        <label htmlFor={"agent-name-" + agent.id}>名称</label>
                        <input
                          id={"agent-name-" + agent.id}
                          value={draft.name}
                          onChange={(event) => updateAgentDraft(agent, { name: event.target.value })}
                          placeholder="Agent 名称"
                        />
                      </div>
                      <div className="form-field">
                        <label htmlFor={"agent-role-" + agent.id}>职责</label>
                        <input
                          id={"agent-role-" + agent.id}
                          value={draft.role}
                          onChange={(event) => updateAgentDraft(agent, { role: event.target.value })}
                          placeholder="Agent 职责"
                        />
                      </div>
                    </div>
                    <label className="checkbox-field agent-global-toggle">
                      <input
                        type="checkbox"
                        checked={draft.uses_global_runtime_settings}
                        onChange={(event) =>
                          updateAgentDraft(agent, {
                            uses_global_runtime_settings: event.target.checked,
                          })
                        }
                      />
                      继承全局供应商、模型和思考配置
                    </label>
                    {draft.uses_global_runtime_settings && (
                      <p className="agent-inherited-summary">
                        当前生效：{aiSettings.model || "未设置模型"} · 思考{aiSettings.thinking_enabled ? ({ low: "低", medium: "中", high: "高", off: "关闭" }[aiSettings.thinking_level]) : "关闭"}
                      </p>
                    )}
                    <div className="agent-runtime-grid">
                      <div className="form-field">
                        <label htmlFor={`agent-provider-${agent.id}`}>供应商</label>
                        <Select
                          id={`agent-provider-${agent.id}`}
                          value={draft.provider_base_url}
                          disabled={draft.uses_global_runtime_settings}
                          onChange={(value) => {
                            const provider = agentProviderOptions.find(
                              (item) => item.base_url === value
                            );
                            updateAgentDraft(agent, {
                              provider_base_url: value,
                              model: provider?.model || draft.model,
                            });
                          }}
                          options={agentProviderOptions.map((provider) => ({
                            value: provider.base_url,
                            label: `${provider.label} · ${provider.base_url || "未填写地址"}`,
                          }))}
                        />
                      </div>
                      <div className="form-field">
                        <label htmlFor={`agent-model-${agent.id}`}>模型</label>
                        <div className="model-picker-row">
                          <Select
                            className="agent-model-select"
                            value={customAgentModel ? "" : draft.model}
                            disabled={draft.uses_global_runtime_settings}
                            onChange={(model) => updateAgentDraft(agent, { model })}
                            options={[
                              { value: "", label: "自定义模型" },
                              ...availableModels.map((model) => ({
                                value: model.id,
                                label: `${model.id}${model.owned_by ? ` · ${model.owned_by}` : ""}`,
                              })),
                            ]}
                          />
                          <button
                            type="button"
                            onClick={() => void refreshAgentModels(agent)}
                            disabled={
                              loadingAgentModels === agent.id ||
                              !draft.provider_base_url.trim() ||
                              draft.uses_global_runtime_settings
                            }
                            title="刷新该供应商的模型列表"
                          >
                            <RefreshCcw size={14} className={loadingAgentModels === agent.id ? "spin" : undefined} />
                          </button>
                        </div>
                        {customAgentModel && (
                          <input
                            id={`agent-model-${agent.id}`}
                            value={draft.model}
                            disabled={draft.uses_global_runtime_settings}
                            onChange={(event) => updateAgentDraft(agent, { model: event.target.value })}
                            placeholder="模型名称"
                          />
                        )}
                      </div>
                    </div>
                    <label className="checkbox-field agent-thinking-toggle">
                      <input
                        type="checkbox"
                        checked={draft.thinking_enabled}
                        disabled={draft.uses_global_runtime_settings}
                        onChange={(event) => {
                          const thinking_enabled = event.target.checked;
                          updateAgentDraft(agent, {
                            thinking_enabled,
                            thinking_level: thinking_enabled
                              ? (draft.thinking_level === "off" ? "medium" : draft.thinking_level)
                              : "off",
                          });
                        }}
                      />
                      启用深度思考
                    </label>
                    <div className="agent-runtime-grid">
                      <div className="form-field">
                        <label htmlFor={"agent-temperature-" + agent.id}>Temperature</label>
                        <input
                          id={"agent-temperature-" + agent.id}
                          type="number"
                          min="0"
                          max="2"
                          step="0.05"
                          value={draft.temperature}
                          onChange={(event) =>
                            updateAgentDraft(agent, { temperature: Number(event.target.value) })
                          }
                        />
                      </div>
                      <div className="form-field">
                        <label htmlFor={"agent-thinking-level-" + agent.id}>思考强度</label>
                        <Select
                          id={"agent-thinking-level-" + agent.id}
                          value={draft.thinking_enabled ? draft.thinking_level : "off"}
                          disabled={!draft.thinking_enabled || draft.uses_global_runtime_settings}
                          onChange={(value) =>
                            updateAgentDraft(agent, {
                              thinking_level: value as ThinkingLevel,
                            })
                          }
                          options={[
                            { value: "off", label: "关闭" },
                            { value: "low", label: "低" },
                            { value: "medium", label: "中" },
                            { value: "high", label: "高" },
                          ]}
                        />
                      </div>
                    </div>
                    <div className="agent-allowlist-grid">
                      <div>
                        <label>可用工具</label>
                        <div className="agent-checkbox-list">
                          {agentTools.map((tool) => (
                            <label className="checkbox-field" key={tool.key}>
                              <input
                                type="checkbox"
                                checked={draft.enabled_tool_keys.includes(tool.key)}
                                onChange={(event) =>
                                  updateAgentDraft(agent, {
                                    enabled_tool_keys: event.target.checked
                                      ? [...new Set([...draft.enabled_tool_keys, tool.key])]
                                      : draft.enabled_tool_keys.filter((key) => key !== tool.key),
                                  })
                                }
                              />
                              <span>
                                {tool.name}
                                <small>{tool.category}</small>
                              </span>
                            </label>
                          ))}
                        </div>
                      </div>
                      <div>
                        <label>辅助 Skill</label>
                        <div className="agent-checkbox-list">
                          {writingSkills
                            .filter((skill) => skill.enabled)
                            .map((skill) => (
                              <label className="checkbox-field" key={skill.skill_key}>
                                <input
                                  type="checkbox"
                                  checked={draft.allowed_skill_keys.includes(skill.skill_key)}
                                  onChange={(event) =>
                                    updateAgentDraft(agent, {
                                      allowed_skill_keys: event.target.checked
                                        ? [...new Set([...draft.allowed_skill_keys, skill.skill_key])]
                                        : draft.allowed_skill_keys.filter((key) => key !== skill.skill_key),
                                    })
                                  }
                                />
                                <span>
                                  {skill.name}
                                  <small>{skill.skill_key}</small>
                                </span>
                              </label>
                            ))}
                        </div>
                      </div>
                    </div>
                    <div className="button-row agent-settings-actions">
                      <button
                        type="button"
                        onClick={() => void saveAgentDraft(agent)}
                        disabled={
                          Boolean(busy) ||
                          !draft.name.trim() ||
                          !draft.role.trim() ||
                          !draft.system_prompt.trim() ||
                          !Number.isFinite(draft.temperature) ||
                          draft.temperature < 0 ||
                          draft.temperature > 2 ||
                          (!draft.uses_global_runtime_settings &&
                            (!draft.provider_base_url.trim() || !draft.model.trim()))
                        }
                        className="btn-primary"
                      >
                        {busy === "保存 Agent 配置" ? <Loader2 size={14} className="spin" /> : <><Check size={14} /> 保存 Agent 配置</>}
                      </button>
                      <button
                        type="button"
                        onClick={() => void resetAgentPrompt(agent)}
                        disabled={Boolean(busy)}
                      >
                        <RefreshCcw size={14} /> 恢复 V2 默认 Prompt
                      </button>
                    </div>
                    <label htmlFor={`agent-prompt-${agent.id}`}>系统提示词</label>
                    <textarea
                      id={`agent-prompt-${agent.id}`}
                      className="agent-prompt-textarea"
                      value={draft.system_prompt}
                      onChange={(event) => updateAgentDraft(agent, { system_prompt: event.target.value })}
                      spellCheck={false}
                    />
                  </section>
                );
              })()}
              {!selectedAgent && <p className="settings-hint">未选择项目</p>}
              </div>
            </div>
          </div>
        );

      case "editor":
        return (
          <div className="settings-content">
            <div className="settings-section">
              <h3>编辑器偏好</h3>
              <p className="settings-hint">暂无设置</p>
            </div>
          </div>
        );

      case "skills":
        return (
          <div className="settings-content wide">
            <div className="settings-section">
              <h3>写作技能库</h3>
              <div className="skill-editor-layout">
                <div className="skill-list">
                  {writingSkills.map((skill) => (
                    <button
                      key={skill.skill_key}
                      type="button"
                      className={selectedSkillKey === skill.skill_key ? "skill-list-item active" : "skill-list-item"}
                      onClick={() => setSelectedSkillKey(skill.skill_key)}
                    >
                      <strong>{skill.name}</strong>
                      <span>{skill.skill_key}</span>
                    </button>
                  ))}
                </div>

                {skillDraft ? (
                  <div className="skill-editor">
                    <div className="form-grid two">
                      <div className="form-field">
                        <label htmlFor="skill_name">名称</label>
                        <input
                          id="skill_name"
                          value={skillDraft.name}
                          onChange={(e) => setSkillDraft({ ...skillDraft, name: e.target.value })}
                        />
                      </div>
                      <div className="form-field">
                        <label htmlFor="skill_key">标识</label>
                        <input
                          id="skill_key"
                          value={skillDraft.skill_key}
                          readOnly
                        />
                      </div>
                    </div>
                    <div className="form-field">
                      <label htmlFor="skill_description">说明</label>
                      <input
                        id="skill_description"
                        value={skillDraft.description}
                        onChange={(e) => setSkillDraft({ ...skillDraft, description: e.target.value })}
                      />
                    </div>
                    <div className="form-field checkbox-field">
                      <label>
                        <input
                          type="checkbox"
                          checked={skillDraft.enabled}
                          onChange={(e) => setSkillDraft({ ...skillDraft, enabled: e.target.checked })}
                        />
                        启用
                      </label>
                    </div>
                    <div className="form-field">
                      <label htmlFor="skill_content">Markdown 规则</label>
                      <textarea
                        id="skill_content"
                        className="skill-textarea"
                        value={skillDraft.content}
                        onChange={(e) => setSkillDraft({ ...skillDraft, content: e.target.value })}
                        spellCheck={false}
                      />
                    </div>
                    <div className="button-row">
                      <button onClick={handleSaveSkill} disabled={Boolean(busy)} className="btn-primary">
                        {busy === "保存技能" ? <Loader2 size={14} className="spin" /> : <><KeyRound size={14} /> 保存技能</>}
                      </button>
                    </div>
                  </div>
                ) : (
                  <p className="settings-hint">暂无技能</p>
                )}
              </div>
            </div>
          </div>
        );

      case "data":
        return (
          <div className="settings-content">
            <div className="settings-section">
              <h3>数据管理</h3>
              <div className="local-search-status">
                <div className="local-search-status-header">
                  <div>
                    <strong>本地混合检索</strong>
                    <span>bge-small-zh-v1.5 · FTS5 trigram · sqlite-vec</span>
                  </div>
                  <button
                    type="button"
                    onClick={handleRebuildStorySearch}
                    disabled={!projectId || Boolean(busy)}
                    title="重建当前项目的本地检索索引"
                  >
                    <RefreshCcw size={14} />
                    重建
                  </button>
                </div>
                {localStorySearchStatus ? (
                  <div className="local-search-status-grid">
                    <span>模型</span>
                    <strong>{localStorySearchStatus.model_status}</strong>
                    <span>sqlite-vec</span>
                    <strong>{localStorySearchStatus.sqlite_vec_status}</strong>
                    <span>索引</span>
                    <strong>
                      {localStorySearchStatus.document_count} 个片段 · {localStorySearchStatus.embedding_count} 个向量
                    </strong>
                    <span>最近更新</span>
                    <strong>{localStorySearchStatus.last_indexed_at ?? "尚未建立"}</strong>
                  </div>
                ) : (
                  <p className="settings-hint">{projectId ? "读取中…" : "未选择项目"}</p>
                )}
                {localStorySearchStatus?.stale && (
                  <p className="local-search-stale">待重建：{localStorySearchStatus.stale_sources}</p>
                )}
              </div>
            </div>
          </div>
        );

      case "appearance":
        return (
          <div className="settings-content">
            <div className="settings-section">
              <h3>外观设置</h3>
              <p className="settings-hint">暂无设置</p>
            </div>
          </div>
        );

      default:
        return null;
    }
  };

  return (
    <div className="settings-view">
      {testConnectionToast && (
        <div className="settings-toast settings-toast-success" role="status" aria-live="polite">
          <Check size={14} />
          <span>{testConnectionToast}</span>
        </div>
      )}
      {errorToast && (
        <div className="settings-toast settings-toast-error" role="alert" aria-live="assertive">
          <AlertCircle size={14} />
          <span>{errorToast}</span>
        </div>
      )}
      <aside className="settings-sidebar">
        <header className="settings-sidebar-header">
          <button className="back-btn" onClick={onBack}>
            <ArrowLeft size={16} />
            返回主菜单
          </button>
        </header>
        <nav className="settings-nav">
          <div className="settings-nav-label">设置分组</div>
          {categories.map((cat) => (
            <button
              key={cat.id}
              className={settingsCategory === cat.id ? "nav-item active" : "nav-item"}
              onClick={() => onSettingsCategoryChange(cat.id)}
            >
              {cat.icon}
              <span>{cat.label}</span>
            </button>
          ))}
        </nav>
      </aside>

      <main className="settings-main">
        <header className="settings-main-header">
          <h2>{categories.find((c) => c.id === settingsCategory)?.label}</h2>
        </header>
        <div className="settings-main-body">
          {(busy && !isTestingConnection) || (notice && !isConnectionSuccessNotice) ? (
            <div className={`status-banner ${error ? "error" : ""}`}>
              {busy && !isTestingConnection && <Loader2 size={14} className="spin" />}
              {notice && !error && !busy && <Check size={14} />}
              <span>{busy ?? error ?? notice}</span>
            </div>
          ) : null}
          {renderCategoryContent()}
        </div>
      </main>
    </div>
  );
}
