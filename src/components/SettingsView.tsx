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
import { useState, useEffect } from "react";
import type {
  Agent,
  AiSettings,
  GenreAgentProfile,
  ModelInfo,
  SaveWritingSkill,
  StorySearchStatus,
  WritingSkill,
} from "../types";

type SettingsCategory = "ai" | "agents" | "skills" | "editor" | "data" | "appearance";

type ProviderConfig = {
  id: string;
  label: string;
  baseUrl: string;
  model: string;
  temperature: number;
  thinkingEnabled: boolean;
};

const PROVIDER_STORAGE_KEY = "xiic-book-studio.ai-providers.v1";
const defaultProviderConfigs: ProviderConfig[] = [
  {
    id: "deepseek",
    label: "DeepSeek",
    baseUrl: "https://api.deepseek.com",
    model: "deepseek-v4-pro",
    temperature: 0.75,
    thinkingEnabled: false,
  },
  {
    id: "minimax",
    label: "MiniMax",
    baseUrl: "https://api.minimaxi.com/v1",
    model: "MiniMax-M3",
    temperature: 0.72,
    thinkingEnabled: true,
  },
];

function makeProviderId(seed: string) {
  const normalized = seed
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return normalized || `provider-${Date.now()}`;
}

function guessProviderLabel(baseUrl: string) {
  const normalized = baseUrl.toLowerCase();
  if (normalized.includes("deepseek")) return "DeepSeek";
  if (normalized.includes("minimax") || normalized.includes("minimaxi")) return "MiniMax";
  if (normalized.includes("openai")) return "OpenAI";
  return "自定义供应商";
}

function normalizeBaseUrl(baseUrl: string) {
  return baseUrl.trim();
}

function normalizeProvider(provider: Partial<ProviderConfig>, index: number): ProviderConfig {
  return {
    id: provider.id?.trim() || `provider-${index + 1}`,
    label: provider.label?.trim() || `供应商 ${index + 1}`,
    baseUrl: normalizeBaseUrl(provider.baseUrl ?? ""),
    model: provider.model?.trim() || "",
    temperature: Number.isFinite(provider.temperature) ? Number(provider.temperature) : 0.75,
    thinkingEnabled: Boolean(provider.thinkingEnabled),
  };
}

function readStoredProviders() {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(PROVIDER_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.map((provider, index) => normalizeProvider(provider, index));
  } catch {
    return [];
  }
}

function buildProviderFromSettings(settings: AiSettings): ProviderConfig {
  const baseUrl = normalizeBaseUrl(settings.base_url);
  const preset = defaultProviderConfigs.find(
    (provider) => provider.baseUrl.trim() === baseUrl
  );
  return {
    id: preset?.id ?? makeProviderId(baseUrl || guessProviderLabel(baseUrl)),
    label: preset?.label ?? guessProviderLabel(settings.base_url),
    baseUrl,
    model: settings.model,
    temperature: settings.temperature,
    thinkingEnabled: settings.thinking_enabled,
  };
}

function normalizeAiSettings(settings: AiSettings): AiSettings {
  return {
    ...settings,
    base_url: normalizeBaseUrl(settings.base_url),
  };
}

function buildProviderState(settings: AiSettings) {
  const normalizedSettings = normalizeAiSettings(settings);
  const storedProviders = readStoredProviders();
  const baseProviders = (storedProviders.length > 0 ? storedProviders : defaultProviderConfigs).map(
    (provider, index) => normalizeProvider(provider, index)
  );
  const activeBaseUrl = normalizedSettings.base_url.trim();
  const matchedProvider = baseProviders.find((provider) => provider.baseUrl.trim() === activeBaseUrl);

  if (matchedProvider) {
    const providers = baseProviders.map((provider) =>
      provider.id === matchedProvider.id
          ? {
            ...provider,
            baseUrl: normalizedSettings.base_url,
            model: normalizedSettings.model,
            temperature: normalizedSettings.temperature,
            thinkingEnabled: normalizedSettings.thinking_enabled,
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
  projectId?: number | null;
  storySearchStatus?: StorySearchStatus | null;
  apiKey: string;
  settingsCategory: SettingsCategory;
  onSettingsCategoryChange: (cat: SettingsCategory) => void;
  onBack: () => void;
  onSaveSettings: (settings: AiSettings, key: string) => Promise<void>;
  onTestConnection: (settings: AiSettings, key: string) => Promise<void>;
  onRefreshModels: (input?: { base_url?: string | null; api_key?: string | null }) => Promise<ModelInfo[]>;
  onRefreshStorySearchStatus: (projectId?: number | null) => Promise<StorySearchStatus | null>;
  onRebuildStorySearch: () => Promise<void>;
  agents: Agent[];
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
  projectId,
  storySearchStatus,
  apiKey,
  settingsCategory,
  onSettingsCategoryChange,
  onBack,
  onSaveSettings,
  onTestConnection,
  onRefreshModels,
  onRefreshStorySearchStatus,
  onRebuildStorySearch,
  agents,
  genreAgent,
  writingSkills,
  onSaveWritingSkill,
  busy,
  notice,
  error,
}: SettingsViewProps) {
  const initialProviderState = buildProviderState(settings);
  const [aiSettings, setAiSettings] = useState<AiSettings>(settings);
  const [localApiKey, setLocalApiKey] = useState(apiKey);
  const [providers, setProviders] = useState<ProviderConfig[]>(initialProviderState.providers);
  const [selectedProviderId, setSelectedProviderId] = useState(initialProviderState.selectedProviderId);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [loadingModels, setLoadingModels] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);
  const [selectedSkillKey, setSelectedSkillKey] = useState("");
  const [skillDraft, setSkillDraft] = useState<SaveWritingSkill | null>(null);
  const [localStorySearchStatus, setLocalStorySearchStatus] = useState<StorySearchStatus | null>(
    storySearchStatus ?? null
  );

  useEffect(() => {
    const nextProviderState = buildProviderState(settings);
    setAiSettings(normalizeAiSettings(settings));
    setLocalApiKey(apiKey);
    setProviders(nextProviderState.providers);
    setSelectedProviderId(nextProviderState.selectedProviderId);
    setModels([]);
    setModelError(null);
  }, [settings, apiKey]);

  useEffect(() => {
    setLocalStorySearchStatus(storySearchStatus ?? null);
  }, [storySearchStatus]);

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
    if (typeof window === "undefined") return;
    window.localStorage.setItem(PROVIDER_STORAGE_KEY, JSON.stringify(providers));
  }, [providers]);

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

  const handleSaveAi = async () => {
    await onSaveSettings(aiSettings, localApiKey);
    setLocalApiKey("");
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

  const handleRebuildStorySearch = async () => {
    await onRebuildStorySearch();
    const status = await onRefreshStorySearchStatus(projectId);
    setLocalStorySearchStatus(status);
  };

  const updateCurrentProvider = (patch: Partial<ProviderConfig>) => {
    setProviders((current) =>
      current.map((provider, index) =>
        provider.id === selectedProviderId
          ? normalizeProvider({ ...provider, ...patch }, index)
          : provider
      )
    );
  };

  const selectProvider = (providerId: string) => {
    const provider = providers.find((item) => item.id === providerId);
    if (!provider) return;
    setSelectedProviderId(providerId);
    setAiSettings((current) => ({
      ...current,
      base_url: provider.baseUrl,
      model: provider.model,
      temperature: provider.temperature,
      thinking_enabled: provider.thinkingEnabled,
      has_api_key: current.base_url.trim() === provider.baseUrl.trim() ? current.has_api_key : false,
    }));
    setLocalApiKey("");
    setModels([]);
    setModelError(null);
  };

  const addProvider = () => {
    const nextIndex = providers.length + 1;
    const provider: ProviderConfig = {
      id: `provider-${Date.now()}`,
      label: `自定义供应商 ${nextIndex}`,
      baseUrl: "",
      model: "",
      temperature: aiSettings.temperature,
      thinkingEnabled: aiSettings.thinking_enabled,
    };
    setProviders((current) => [...current, provider]);
    setSelectedProviderId(provider.id);
    setAiSettings((current) => ({
      ...current,
      base_url: provider.baseUrl,
      model: provider.model,
      temperature: provider.temperature,
      thinking_enabled: provider.thinkingEnabled,
      has_api_key: false,
    }));
    setLocalApiKey("");
    setModels([]);
    setModelError(null);
  };

  const removeProvider = (providerId: string) => {
    if (providers.length <= 1) return;
    const nextProviders = providers.filter((provider) => provider.id !== providerId);
    setProviders(nextProviders);
    if (selectedProviderId !== providerId) return;
    const fallbackProvider = nextProviders[0];
    if (!fallbackProvider) return;
    setSelectedProviderId(fallbackProvider.id);
    setAiSettings((current) => ({
      ...current,
      base_url: fallbackProvider.baseUrl,
      model: fallbackProvider.model,
      temperature: fallbackProvider.temperature,
      thinking_enabled: fallbackProvider.thinkingEnabled,
      has_api_key: false,
    }));
    setLocalApiKey("");
    setModels([]);
    setModelError(null);
  };

  const renderCategoryContent = () => {
    switch (settingsCategory) {
      case "ai":
        return (
          <div className="settings-content">
            <div className="settings-intro">
              <p>集中管理 AI 服务供应商、模型参数和访问凭据。</p>
              <span>配置只影响本地工作台，不会写入书籍项目文件。</span>
            </div>
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
                          onClick={() => removeProvider(provider.id)}
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
                  placeholder="例如：DeepSeek / MiniMax / 自定义"
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
                    updateCurrentProvider({ baseUrl: value });
                  }}
                  placeholder="https://api.deepseek.com"
                />
              </div>
              <div className="form-field">
                <label htmlFor="model">模型名称</label>
                <div className="model-picker">
                  <div className="model-picker-row">
                    <select
                      id="model"
                      value={aiSettings.model}
                      onChange={(e) => {
                        const value = e.target.value;
                        setAiSettings({ ...aiSettings, model: value });
                        updateCurrentProvider({ model: value });
                      }}
                    >
                      <option value="">手动输入模型名</option>
                      {(aiSettings.model && !models.some((model) => model.id === aiSettings.model)
                        ? [{ id: aiSettings.model, owned_by: null }, ...models]
                        : models
                      ).map((model) => (
                        <option key={model.id} value={model.id}>
                          {model.id}
                          {model.owned_by ? ` · ${model.owned_by}` : ""}
                        </option>
                      ))}
                    </select>
                    <button type="button" onClick={handleRefreshModels} disabled={loadingModels || Boolean(busy)}>
                      {loadingModels ? <Loader2 size={14} className="spin" /> : <ChevronDown size={14} />}
                      刷新
                    </button>
                  </div>
                  <input
                    type="text"
                    value={aiSettings.model}
                    onChange={(e) => {
                      const value = e.target.value;
                      setAiSettings({ ...aiSettings, model: value });
                      updateCurrentProvider({ model: value });
                    }}
                    placeholder="也可直接手动输入模型名"
                  />
                  <div className="settings-hint-row">
                    <span>模型列表通过当前 Base URL 的 `/models` 手动刷新。</span>
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
                      setAiSettings({ ...aiSettings, thinking_enabled: value });
                      updateCurrentProvider({ thinkingEnabled: value });
                    }}
                  />
                  <span>开启供应商支持的思考能力</span>
                </label>
                <div className="settings-hint-row">
                  <span>DeepSeek 与 MiniMax 会自动使用各自协议；未知供应商默认不发送思考参数。</span>
                </div>
              </div>
              <div className="form-field">
                <label htmlFor="api_key">API Key</label>
                <input
                  id="api_key"
                  type="password"
                  value={localApiKey}
                  placeholder="输入新 Key，留空则保留该供应商已保存的 Key"
                  onChange={(e) => setLocalApiKey(e.target.value)}
                />
                <div className="settings-hint-row">
                  <span>API Key 会按供应商单独保存，MiniMax 和 DeepSeek 不会再串用。</span>
                </div>
              </div>
              <div className="button-row">
                <button onClick={handleSaveAi} disabled={Boolean(busy)} className="btn-primary">
                  {busy === "保存设置" ? <Loader2 size={14} className="spin" /> : <><KeyRound size={14} /> 保存</>}
                </button>
                <button onClick={handleTestConnection} disabled={Boolean(busy)}>
                  <RefreshCcw size={14} /> 测试连接
                </button>
              </div>
            </div>
          </div>
        );

      case "agents":
        return (
          <div className="settings-content wide">
            <div className="settings-intro">
              <p>每本书持久绑定一个题材专属 Agent；故事架构、写作、试读和修订是它的工作模式。</p>
              <span>专属 Agent 只能加载白名单 Skill，技能库中的其他题材规则不会进入上下文。</span>
            </div>
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
            <div className="agent-prompt-list">
              {agents.map((agent) => (
                <section className="agent-prompt-card" key={agent.id}>
                  <header>
                    <div>
                      <strong>{agent.name}</strong>
                      <span>{agent.stage}</span>
                    </div>
                    <small>Temperature {agent.temperature}</small>
                  </header>
                  <p>{agent.role}</p>
                  <label htmlFor={`agent-prompt-${agent.id}`}>系统提示词</label>
                  <textarea
                    id={`agent-prompt-${agent.id}`}
                    className="agent-prompt-textarea"
                    readOnly
                    value={agent.system_prompt}
                    spellCheck={false}
                  />
                </section>
              ))}
              {agents.length === 0 && <p className="settings-hint">选择一个书籍项目后查看 Agent 配置。</p>}
            </div>
          </div>
        );

      case "editor":
        return (
          <div className="settings-content">
            <div className="settings-section">
              <h3>编辑器偏好</h3>
              <p className="settings-hint">编辑器相关设置将在后续版本中开放</p>
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
                          onChange={(e) => setSkillDraft({ ...skillDraft, skill_key: e.target.value })}
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
                  <p className="settings-hint">技能库还没有内容</p>
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
                  <p className="settings-hint">
                    {projectId ? "正在读取当前项目的本地检索状态" : "选择书籍项目后查看本地检索状态"}
                  </p>
                )}
                {localStorySearchStatus?.stale && (
                  <p className="local-search-stale">
                    有 {localStorySearchStatus.stale_sources} 个来源因正文更新而等待重建。
                  </p>
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
              <p className="settings-hint">主题、字体大小等设置将在后续版本中开放</p>
            </div>
          </div>
        );

      default:
        return null;
    }
  };

  return (
    <div className="settings-view">
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
          {(notice || error || busy) && (
            <div className={`status-banner ${error ? "error" : ""}`}>
              {busy && <Loader2 size={14} className="spin" />}
              {error && <AlertCircle size={14} />}
              {notice && !error && !busy && <Check size={14} />}
              <span>{busy ?? error ?? notice}</span>
            </div>
          )}
          {renderCategoryContent()}
        </div>
      </main>
    </div>
  );
}
