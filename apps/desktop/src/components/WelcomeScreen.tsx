import { Eye, Image as ImageIcon, Images, LockKeyhole, Plus, ShieldCheck, Undo2 } from "lucide-react";
import { resolveAssetUrl } from "../api/assets";
import { formatRelativeTime } from "../lib/format";
import { useEditorStore } from "../stores/editorStore";
import { Button } from "./controls/Button";

export function WelcomeScreen() {
  const openPhoto = useEditorStore((state) => state.openPhoto);
  const loadSample = useEditorStore((state) => state.loadSample);
  const openCollection = useEditorStore((state) => state.openCollection);
  const openNewProject = useEditorStore((state) => state.openNewProject);
  const collections = useEditorStore((state) => state.collections);
  const importBusy = useEditorStore((state) => state.importBusy);
  const sampleBusy = useEditorStore((state) => state.sampleBusy);
  const newProjectBusy = useEditorStore((state) => state.newProjectBusy);
  const mode = useEditorStore((state) => state.mode);

  const busy = importBusy || sampleBusy || newProjectBusy;

  return (
    <main className="welcome">
      <div className="welcome__glow" aria-hidden="true" />
      <div className="welcome__inner">
        <section className="welcome__copy">
          <p className="eyebrow">
            <span className="eyebrow__line" aria-hidden="true" />
            LOCAL-FIRST · NON-DESTRUCTIVE
          </p>
          <h1 className="welcome__title">
            让每一张人像，
            <br />
            <span>自然好看</span>
          </h1>
          <p className="welcome__lead">
            不用记参数，也不用把照片交给陌生人。选一个喜欢的风格，按住就能和原图对比。
            所有调整都留在你的电脑里，随时可以反悔。
          </p>

          <div className="welcome__actions">
            <Button
              variant="primary"
              size="lg"
              icon={<Plus size={16} />}
              loading={newProjectBusy}
              disabled={busy}
              onClick={openNewProject}
            >
              新建项目
            </Button>
            <Button
              variant="secondary"
              size="lg"
              icon={<ImageIcon size={16} />}
              loading={importBusy}
              disabled={busy}
              onClick={() => void openPhoto()}
            >
              打开照片
            </Button>
            <Button
              variant="ghost"
              size="lg"
              icon={<Images size={16} />}
              loading={sampleBusy}
              disabled={busy}
              onClick={() => void loadSample()}
            >
              试用示例
            </Button>
          </div>
          <p className="welcome__drop-hint">
            打开照片支持多选，也可以拖进窗口 · JPG、PNG、TIFF、WebP · RAW 仅桌面版（最大 200 MB）
          </p>

          <ul className="welcome__features">
            <li>
              <ShieldCheck size={13} aria-hidden="true" />
              照片只在本机处理
            </li>
            <li>
              <Undo2 size={13} aria-hidden="true" />
              每一步都能撤销
            </li>
            <li>
              <LockKeyhole size={13} aria-hidden="true" />
              原图永不覆盖
            </li>
          </ul>

          <div className="welcome__recent">
            <div className="welcome__recent-head">
              <span>最近项目</span>
              <span className="welcome__recent-count">{collections.length > 0 ? `${collections.length} 个` : "暂无"}</span>
            </div>
            {collections.length === 0 ? (
              <p className="welcome__recent-empty">新建项目并导入照片后，这里会保留最近的照片集。</p>
            ) : (
              <div className="recent-list">
                {collections.slice(0, 3).map((collection) => {
                  const thumb = resolveAssetUrl(collection.coverThumbnailPath);
                  return (
                    <button
                      key={collection.id}
                      type="button"
                      className="recent-row"
                      disabled={busy}
                      onClick={() => void openCollection(collection.id)}
                      title={`打开 ${collection.name}`}
                    >
                      <span className="recent-row__thumb">
                        {thumb ? <img src={thumb} alt="" /> : <ImageIcon size={14} />}
                      </span>
                      <span className="recent-row__copy">
                        <span className="recent-row__name">{collection.name}</span>
                        <span className="recent-row__meta">
                          {collection.photoCount} 张照片 · {formatRelativeTime(collection.updatedAt)}
                        </span>
                      </span>
                      <span className="recent-row__action">继续编辑</span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>

          <p className="welcome__mode">
            {mode === "demo"
              ? "浏览器演示模式 · 所有效果均在本地近似计算"
              : "Windows 桌面版 · 本地 CPU 图像管线"}
          </p>
        </section>

        <section className="welcome__visual" aria-label="示例照片">
          <div className="visual-glow" aria-hidden="true" />
          <div className="visual-frame">
            <div className="visual-frame__top">
              <span>自然光 · 人像示例</span>
              <span className="mono">768 × 966</span>
            </div>
            <div className="visual-frame__image">
              <img src="/samples/portrait.jpg" alt="示例人像照片，自然光下的工作室肖像" />
              <span className="visual-frame__tag">
                <Eye size={11} aria-hidden="true" />
                按住对比原图
              </span>
            </div>
            <div className="visual-frame__bottom">
              <span>演示照片</span>
              <span>所有调整以图层保存</span>
            </div>
          </div>
          <p className="visual-caption">
            从这张示例开始，试试「自然美颜」和「清透提亮」的区别。
          </p>
        </section>
      </div>
    </main>
  );
}
