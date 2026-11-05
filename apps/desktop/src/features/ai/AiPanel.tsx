import { useEffect, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  History,
  KeyRound,
  Plus,
  ShieldCheck,
  Sparkles,
  Square,
  WandSparkles,
  X,
} from "lucide-react";
import type { AiJobInfo, AiJobStatus, ProviderInfo, ProviderTestResult } from "../../api/types";
import { resolveAssetUrl } from "../../api/assets";
import { cn } from "../../lib/cn";
import { formatRelativeTime } from "../../lib/format";
import { isLoopbackProviderUrl } from "../../lib/providerUrl";
import { useEditorStore } from "../../stores/editorStore";
import { PanelHeader } from "../../components/PanelHeader";
import { Button } from "../../components/controls/Button";
import { EmptyState } from "../../components/controls/EmptyState";
import { Segmented } from "../../components/controls/Segmented";
import { Spinner } from "../../components/controls/Spinner";

const PROMPT_EXAMPLES = [
  "让肤色更均匀，保留皮肤纹理",
  "背景柔和一些，突出人物",
  "整体提亮，干净通透的影棚感",
];

const STRENGTH_OPTIONS = [
  { value: "35", label: "轻", title: "接近原图，只做轻微变化" },
  { value: "65", label: "中", title: "推荐的平衡程度" },
  { value: "90", label: "强", title: "更明显的风格变化" },
];

const STATUS_LABEL: Record<AiJobStatus, string> = {
  queued: "排队中",
  running: "生成中",
  completed: "已完成",
  failed: "未完成",
  cancelled: "已取消",
};

function ProviderBadge({ provider }: { provider: ProviderInfo }) {
  if (provider.kind === "mock") {
    return (
      <span className="badge badge--green" title="所有处理都在本地完成">
        不上传图片
      </span>
    );
  }
  return (
    <span className="badge badge--amber" title="生成时需要把当前照片发送给该服务">
      云端服务
    </span>
  );
}

interface JobCardProps {
  job: AiJobInfo;
  applied: boolean;
  applying: boolean;
  onApply: () => void;
  onCancel: () => void;
}

function JobCard({ job, applied, applying, onApply, onCancel }: JobCardProps) {
  const canCancel = job.status === "running" || job.status === "queued";
  const resultUrl = resolveAssetUrl(job.resultPath);
  const time = formatRelativeTime(job.updatedAt);

  return (
    <article className={cn("job-card", `job-card--${job.status}`)}>
      <header className="job-card__head">
        <span className={cn("job-status", `job-status--${job.status}`)}>
          {canCancel ? <Spinner size={10} /> : null}
          {STATUS_LABEL[job.status]}
        </span>
        <span className="job-card__time">{time}</span>
      </header>
      <p className="job-card__prompt">{job.prompt}</p>

      {canCancel ? (
        <div className="job-progress" aria-label={`生成进度 ${job.progress ?? 0}%`}>
          <div
            className={cn("job-progress__bar", job.progress === null && "is-indeterminate")}
            style={job.progress === null ? undefined : { width: `${job.progress}%` }}
          />
        </div>
      ) : null}

      {job.status === "running" ? <p className="job-card__message">{job.message ?? "正在生成…"}</p> : null}

      {job.status === "completed" && resultUrl ? (
        <div className="job-result">
          <img src={resultUrl} alt="AI 生成效果预览" />
          <div className="job-result__copy">
            <p>生成结果会作为可关闭、可撤销的新图层加入。</p>
            <Button
              variant={applied ? "subtle" : "secondary"}
              size="sm"
              loading={applying}
              disabled={applied}
              icon={applied ? <CheckCircle2 size={13} /> : <Sparkles size={13} />}
              onClick={onApply}
            >
              {applied ? "已应用为图层" : "应用为图层"}
            </Button>
          </div>
        </div>
      ) : null}

      {job.status === "failed" ? (
        <p className="job-card__error">
          <AlertCircle size={12} aria-hidden="true" />
          {job.userMessage ?? "这次生成没有完成，原图没有被修改。"}
        </p>
      ) : null}

      {canCancel ? (
        <button type="button" className="job-cancel" onClick={onCancel}>
          <Square size={10} aria-hidden="true" />
          取消生成
        </button>
      ) : null}
    </article>
  );
}

export function AiPanel() {
  const doc = useEditorStore((state) => state.doc);
  const providers = useEditorStore((state) => state.providers);
  const jobs = useEditorStore((state) => state.jobs);
  const refreshProviders = useEditorStore((state) => state.refreshProviders);
  const loadJobs = useEditorStore((state) => state.loadJobs);
  const saveProviderConfig = useEditorStore((state) => state.saveProviderConfig);
  const testProvider = useEditorStore((state) => state.testProvider);
  const listProviderModels = useEditorStore((state) => state.listProviderModels);
  const submitAiJob = useEditorStore((state) => state.submitAiJob);
  const cancelAiJob = useEditorStore((state) => state.cancelAiJob);
  const applyAiJob = useEditorStore((state) => state.applyAiJob);

  const [providerId, setProviderId] = useState("");
  const [prompt, setPrompt] = useState("");
  const [strength, setStrength] = useState("65");
  const [consent, setConsent] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [expandedJobId, setExpandedJobId] = useState<string | null>(null);
  const [applyingJobId, setApplyingJobId] = useState<string | null>(null);

  const [configOpen, setConfigOpen] = useState(false);
  const [creatingProvider, setCreatingProvider] = useState(false);
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [savingConfig, setSavingConfig] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<ProviderTestResult | null>(null);
  const [models, setModels] = useState<string[]>([]);
  const [modelsBusy, setModelsBusy] = useState(false);

  const provider = providers.find((item) => item.id === providerId) ?? providers[0] ?? null;

  useEffect(() => {
    if (providers.length === 0) return;
    if (!providerId || !providers.some((item) => item.id === providerId)) {
      setProviderId(providers[0].id);
    }
  }, [providers, providerId]);

  useEffect(() => {
    if (!provider) return;
    setName(provider.name);
    setBaseUrl(provider.baseUrl ?? "https://api.openai.com/v1");
    setModel(provider.model ?? "");
    setApiKey("");
    setConsent(false);
    setTestResult(null);
    setModels([]);
    setModelsBusy(false);
    setCreatingProvider(false);
    setConfigOpen(provider.kind === "openaiCompatible");
    // Only re-initialise the form when switching provider identities.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [provider?.id]);

  useEffect(() => {
    void loadJobs();
  }, [doc?.id, loadJobs]);

  if (!doc) {
    return (
      <div className="panel">
        <PanelHeader icon={<WandSparkles size={18} />} title="AI 创作" description="先打开一张照片，再来试试 AI" />
        <EmptyState icon={<Sparkles size={20} />} title="还没有照片" description="打开照片后可以使用本地演示或自定义 AI 服务。" />
      </div>
    );
  }

  const isMock = provider?.kind === "mock";
  const consentText = isMock
    ? "我了解：这是本地演示，不会上传照片。"
    : provider
      ? `我同意将当前照片和提示词发送给「${provider.name}」用于生成效果。`
      : "请先选择一个 AI 服务。";
  const canSubmit = Boolean(provider) && prompt.trim().length > 0 && consent && !submitting;

  const openNewProvider = (): void => {
    setCreatingProvider(true);
    setConfigOpen(true);
    setName("我的 AI 服务");
    setBaseUrl("https://api.openai.com/v1");
    setModel("gpt-image-1");
    setApiKey("");
    setTestResult(null);
  };

  const handleSaveProvider = async (): Promise<void> => {
    setSavingConfig(true);
    try {
      const saved = await saveProviderConfig(
        {
          id: creatingProvider ? null : provider?.id ?? null,
          name,
          kind: "openaiCompatible",
          baseUrl: baseUrl.trim() || null,
          model: model.trim() || null,
          allowLocalhost: isLoopbackProviderUrl(baseUrl),
        },
        apiKey,
      );
      if (saved) {
        setApiKey("");
        setCreatingProvider(false);
        setProviderId(saved.id);
        setTestResult(null);
      }
    } finally {
      setSavingConfig(false);
    }
  };

  const handleFetchModels = async (): Promise<void> => {
    if (!provider || creatingProvider || modelsBusy) return;
    setModelsBusy(true);
    try {
      const next = await listProviderModels(provider.id);
      if (next) {
        setModels(next);
        if (next.length === 1 && model.trim().length === 0) setModel(next[0]);
      }
    } finally {
      setModelsBusy(false);
    }
  };

  const handleTest = async (): Promise<void> => {
    if (!provider || creatingProvider) return;
    setTesting(true);
    try {
      const result = await testProvider(provider.id);
      if (result) setTestResult(result);
    } finally {
      setTesting(false);
    }
  };

  const handleSubmit = async (): Promise<void> => {
    if (!provider || !canSubmit) return;
    setSubmitting(true);
    try {
      const jobId = await submitAiJob(provider.id, prompt.trim(), Number(strength));
      if (jobId) {
        setExpandedJobId(jobId);
        setConsent(false);
      }
    } finally {
      setSubmitting(false);
    }
  };

  const handleApply = async (jobId: string): Promise<void> => {
    setApplyingJobId(jobId);
    try {
      await applyAiJob(jobId);
    } finally {
      setApplyingJobId(null);
    }
  };

  const activeJobs = jobs.filter((job) => job.status === "running" || job.status === "queued");
  const featuredJob = jobs.find((job) => job.id === expandedJobId) ?? jobs[0] ?? null;
  const otherActiveJobs = activeJobs.filter((job) => job.id !== featuredJob?.id);
  const historyJobs = jobs
    .filter((job) => job.id !== featuredJob?.id && !activeJobs.some((active) => active.id === job.id))
    .slice(0, 4);
  const appliedAssetIds = new Set(
    doc.layers.filter((layer) => layer.kind === "aiResult" && layer.assetId).map((layer) => layer.assetId as string),
  );
  // Rust stores the result asset id, which is not the job id. Older demo jobs
  // fall back to the job id for backward compatibility.
  const isApplied = (job: AiJobInfo): boolean =>
    appliedAssetIds.has(job.resultAssetId ?? job.id);

  return (
    <div className="panel">
      <PanelHeader
        icon={<WandSparkles size={18} />}
        title="AI 创作"
        description="用一句话，试试另一种人像效果"
      />

      <section className="panel-section">
        <div className="ai-notice">
          <span className="ai-notice__icon">
            <ShieldCheck size={16} />
          </span>
          <div>
            <p className="ai-notice__title">云端 AI 会上传图片，本地调整不会上传</p>
            <p className="ai-notice__text">
              只有你勾选同意并提交后，当前照片与提示词才会发送给所选服务。原图始终保留。
            </p>
          </div>
        </div>
      </section>

      <section className="panel-section">
        <div className="subsection-head">
          <span className="section-title">AI 服务</span>
          <span className="section-hint">{providers.length} 个可用</span>
        </div>

        {providers.length === 0 ? (
          <EmptyState
            compact
            icon={<WandSparkles size={18} />}
            title="还没有可用的 AI 服务"
            description="刷新列表，或先接入自己的 AI 服务。"
            action={
              <Button size="sm" variant="secondary" onClick={() => void refreshProviders()}>
                刷新列表
              </Button>
            }
          />
        ) : (
          <>
            <div className="provider-select">
              <span className="select-wrap">
                <select
                  className="select select--lg"
                  value={provider?.id ?? ""}
                  aria-label="选择 AI 服务"
                  onChange={(event) => setProviderId(event.currentTarget.value)}
                >
                  {providers.map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.name}
                    </option>
                  ))}
                </select>
                <ChevronDown size={13} className="select-wrap__icon" aria-hidden="true" />
              </span>
              {provider ? <ProviderBadge provider={provider} /> : null}
            </div>

            {provider ? (
              <div className="provider-meta">
                <span>{isMock ? "完全本地执行，不联网" : provider.model ?? "未设置模型"}</span>
                {!isMock ? (
                  <span className={cn("provider-credential", provider.hasCredential && "is-ready")}>
                    {provider.hasCredential ? "已保存密钥" : "未保存密钥"}
                  </span>
                ) : null}
              </div>
            ) : null}

            {isMock ? (
              <p className="inline-note inline-note--green">
                <ShieldCheck size={12} aria-hidden="true" />
                当前为本地演示服务，不会上传任何图片，结果由浏览器在本地近似生成。
              </p>
            ) : null}

            <div className="provider-actions">
              {!isMock && provider ? (
                <button
                  type="button"
                  className="panel-link"
                  onClick={() => setConfigOpen((open) => !open)}
                  aria-expanded={configOpen}
                >
                  <KeyRound size={11} aria-hidden="true" />
                  {configOpen ? "收起服务配置" : "编辑服务配置"}
                </button>
              ) : (
                <span />
              )}
              <button
                type="button"
                className="panel-link"
                onClick={openNewProvider}
                disabled={creatingProvider}
              >
                <Plus size={11} aria-hidden="true" />
                接入自定义 AI 服务
              </button>
            </div>

            {configOpen ? (
              <div className="config-form">
                <div className="config-form__head">
                  <span>
                    <KeyRound size={13} aria-hidden="true" />
                    {creatingProvider ? "接入自定义 AI 服务" : "服务配置"}
                  </span>
                  <button
                    type="button"
                    className="config-form__close"
                    aria-label="收起配置"
                    title="收起配置"
                    onClick={() => {
                      setConfigOpen(false);
                      setCreatingProvider(false);
                    }}
                  >
                    <X size={13} />
                  </button>
                </div>

                <label className="field">
                  <span>名称</span>
                  <input
                    className="input"
                    value={name}
                    maxLength={60}
                    placeholder="例如：我的图像服务"
                    onChange={(event) => setName(event.currentTarget.value)}
                  />
                </label>
                <label className="field">
                  <span>服务地址</span>
                  <input
                    className="input"
                    value={baseUrl}
                    spellCheck={false}
                    placeholder="https://api.openai.com/v1"
                    onChange={(event) => setBaseUrl(event.currentTarget.value)}
                  />
                </label>
                <label className="field">
                  <span>模型</span>
                  <input
                    className="input"
                    value={model}
                    spellCheck={false}
                    placeholder="gpt-image-1"
                    onChange={(event) => setModel(event.currentTarget.value)}
                  />
                </label>
                <div className="model-fetch">
                  <Button
                    variant="secondary"
                    size="sm"
                    loading={modelsBusy}
                    disabled={creatingProvider || !provider || savingConfig}
                    onClick={() => void handleFetchModels()}
                  >
                    获取可用模型
                  </Button>
                  {models.length > 0 ? (
                    <span className="select-wrap select-wrap--mini">
                      <select
                        className="select"
                        value={models.includes(model) ? model : ""}
                        aria-label="选择可用模型"
                        onChange={(event) => setModel(event.currentTarget.value)}
                      >
                        <option value="">选择模型</option>
                        {models.map((item) => (
                          <option key={item} value={item}>
                            {item}
                          </option>
                        ))}
                      </select>
                      <ChevronDown size={11} className="select-wrap__icon" aria-hidden="true" />
                    </span>
                  ) : null}
                </div>
                <p className="field-hint">
                  云端服务会从服务地址读取模型列表；本地演示固定使用内置模型。切换模型后请保存配置。
                </p>
                <label className="field">
                  <span>访问密钥</span>
                  <input
                    className="input"
                    type="password"
                    value={apiKey}
                    autoComplete="new-password"
                    spellCheck={false}
                    placeholder={provider?.hasCredential ? "已保存密钥，留空表示保持不变" : "粘贴服务提供的访问密钥"}
                    onChange={(event) => setApiKey(event.currentTarget.value)}
                  />
                </label>
                <p className="field-hint">
                  密钥只发送一次用于保存，之后不会再显示，也不会写入浏览器存储。
                </p>

                {testResult ? (
                  <p className={cn("test-result", testResult.ok ? "is-ok" : "is-fail")}>
                    {testResult.ok ? <CheckCircle2 size={12} /> : <AlertCircle size={12} />}
                    <span>
                      {testResult.message}（{testResult.latencyMs} ms）
                    </span>
                  </p>
                ) : null}

                <div className="config-form__actions">
                  <Button
                    variant="primary"
                    size="sm"
                    loading={savingConfig}
                    disabled={name.trim().length === 0 || testing}
                    onClick={() => void handleSaveProvider()}
                  >
                    保存配置
                  </Button>
                  <Button
                    variant="secondary"
                    size="sm"
                    loading={testing}
                    disabled={creatingProvider || !provider || savingConfig}
                    onClick={() => void handleTest()}
                  >
                    测试连接
                  </Button>
                </div>
                {creatingProvider ? <p className="field-hint">先保存配置，才可测试连接。</p> : null}
              </div>
            ) : null}
          </>
        )}
      </section>

      <section className="panel-section">
        <div className="subsection-head">
          <span className="section-title">想要的效果</span>
          <span className="section-hint">{prompt.length}/500</span>
        </div>
        <textarea
          className="textarea"
          rows={4}
          maxLength={500}
          value={prompt}
          placeholder="描述你想调整的地方，例如：肤色均匀一点，保留皮肤纹理，背景柔和。"
          aria-label="AI 提示词"
          onChange={(event) => setPrompt(event.currentTarget.value)}
        />
        <div className="prompt-examples" role="group" aria-label="提示词示例">
          {PROMPT_EXAMPLES.map((example) => (
            <button key={example} type="button" className="prompt-chip" onClick={() => setPrompt(example)}>
              {example}
            </button>
          ))}
        </div>

        {isMock ? (
          <div className="ai-strength">
            <span className="section-title">调整程度</span>
            <Segmented
              value={strength}
              options={STRENGTH_OPTIONS}
              onChange={setStrength}
              ariaLabel="AI 调整程度"
              size="sm"
            />
          </div>
        ) : (
          <p className="inline-note">
            <AlertCircle size={12} aria-hidden="true" />
            云端服务的强度由提示词和服务本身决定，本版本不发送单独的程度参数。
          </p>
        )}

        <label className={cn("consent-check", !provider && "is-disabled")}>
          <input
            type="checkbox"
            checked={consent}
            disabled={!provider}
            onChange={(event) => setConsent(event.currentTarget.checked)}
          />
          <span>{consentText}</span>
        </label>

        <Button
          variant="primary"
          size="md"
          fullWidth
          icon={<Sparkles size={14} />}
          loading={submitting}
          disabled={!canSubmit}
          onClick={() => void handleSubmit()}
        >
          {provider ? `用「${provider.name}」生成` : "开始生成"}
        </Button>
        <p className="submit-hint">结果会作为新图层加入，随时可以关闭或撤销。</p>
      </section>

      <section className="panel-section">
        <div className="subsection-head">
          <span className="section-title">生成记录</span>
          <span className="section-hint">
            <History size={11} aria-hidden="true" /> 最近 {Math.min(jobs.length, 8)} 条
          </span>
        </div>

        {jobs.length === 0 ? (
          <EmptyState compact icon={<Sparkles size={17} />} title="还没有生成记录" description="描述一个效果，生成结果会出现在这里。" />
        ) : (
          <div className="jobs">
            {featuredJob ? (
              <JobCard
                job={featuredJob}
                applied={isApplied(featuredJob)}
                applying={applyingJobId === featuredJob.id}
                onApply={() => void handleApply(featuredJob.id)}
                onCancel={() => void cancelAiJob(featuredJob.id)}
              />
            ) : null}
            {otherActiveJobs.map((job) => (
              <JobCard
                key={job.id}
                job={job}
                applied={isApplied(job)}
                applying={applyingJobId === job.id}
                onApply={() => void handleApply(job.id)}
                onCancel={() => void cancelAiJob(job.id)}
              />
            ))}
            {historyJobs.length > 0 ? (
              <div className="job-history">
                {historyJobs.map((job) => (
                  <button
                    key={job.id}
                    type="button"
                    className="job-history__row"
                    onClick={() => setExpandedJobId(job.id)}
                    title={job.prompt}
                  >
                    <span className={cn("job-history__dot", `job-history__dot--${job.status}`)} aria-hidden="true" />
                    <span className="job-history__prompt">{job.prompt}</span>
                    <span className="job-history__status">{STATUS_LABEL[job.status]}</span>
                    <span className="job-history__time">{formatRelativeTime(job.updatedAt)}</span>
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        )}
      </section>
    </div>
  );
}
