import {
  Aperture,
  Eye,
  FolderOpen,
  Image as ImageIcon,
  Images,
  LockKeyhole,
} from "lucide-react";
import { resolveAssetUrl } from "../../api/assets";
import { formatDimensions, shortenPath } from "../../lib/format";
import { useEditorStore } from "../../stores/editorStore";
import { PanelHeader } from "../../components/PanelHeader";
import { Button } from "../../components/controls/Button";
import { EmptyState } from "../../components/controls/EmptyState";

export function PhotoPanel() {
  const doc = useEditorStore((state) => state.doc);
  const openPhoto = useEditorStore((state) => state.openPhoto);
  const loadSample = useEditorStore((state) => state.loadSample);
  const importBusy = useEditorStore((state) => state.importBusy);
  const sampleBusy = useEditorStore((state) => state.sampleBusy);

  const source = doc?.source ?? null;
  const thumb = resolveAssetUrl(source?.thumbnailPath);
  const busy = importBusy || sampleBusy;

  return (
    <div className="panel">
      <PanelHeader
        icon={<ImageIcon size={18} />}
        title="照片"
        description="当前照片与文件信息"
      />

      {source ? (
        <>
          <section className="panel-section">
            <div className="photo-card">
              {thumb ? <img src={thumb} alt={`${source.displayName} 缩略图`} /> : null}
              <div className="photo-card__shade" aria-hidden="true" />
              <div className="photo-card__copy">
                <span className="photo-card__name" title={source.displayName}>
                  {source.displayName}
                </span>
                <span className="photo-card__meta">
                  {formatDimensions(source.width, source.height)} · {source.format.toUpperCase()}
                  {["cr2", "cr3", "nef", "nrw", "arw", "dng", "raf", "rw2", "rwl", "orf", "pef", "srw", "raw"].includes(
                    source.format.toLowerCase(),
                  )
                    ? " · RAW"
                    : ""}
                </span>
              </div>
              <span className="photo-card__locked">
                <LockKeyhole size={11} aria-hidden="true" />
                原图只读
              </span>
            </div>
          </section>

          <section className="panel-section">
            <div className="photo-facts">
              <div>
                <span>尺寸</span>
                <strong>{formatDimensions(source.width, source.height)}</strong>
              </div>
              <div>
                <span>格式</span>
                <strong>{source.format.toUpperCase()}</strong>
              </div>
              <div>
                <span>文件名</span>
                <strong title={source.displayName}>{shortenPath(source.displayName, 26)}</strong>
              </div>
              <div>
                <span>色彩空间</span>
                <strong>{source.colorSpace || "sRGB"}</strong>
              </div>
              <div>
                <span>编辑方式</span>
                <strong>非破坏式图层</strong>
              </div>
            </div>
            {source.hasSourceProfile ? (
              <p className="inline-note">
                <Aperture size={12} aria-hidden="true" />
                这张照片带有 {source.colorSpace} 配置，导入时已转换到 sRGB
                工作空间；导出时可以选择「原图配置」写回原来的色彩空间。
              </p>
            ) : null}
            <div className="photo-path">
              <span>文件位置</span>
              <p title={source.originalPath}>{source.originalPath.startsWith("browser://") ? "浏览器演示仅保留在当前会话" : source.originalPath}</p>
            </div>
          </section>

          <section className="panel-section">
            <div className="photo-actions">
              <Button
                variant="secondary"
                size="sm"
                icon={<FolderOpen size={14} />}
                loading={importBusy}
                disabled={busy}
                onClick={() => void openPhoto()}
              >
                打开其他照片
              </Button>
              <Button
                variant="ghost"
                size="sm"
                icon={<Images size={14} />}
                loading={sampleBusy}
                disabled={busy}
                onClick={() => void loadSample()}
              >
                选择示例照片
              </Button>
            </div>
            <div className="compare-hint">
              <span className="compare-hint__icon">
                <Eye size={14} />
              </span>
              <p>
                按住顶部的「按住对比」或画布右上角的按钮，随时查看未修改的原图。松开就回到当前效果。
              </p>
            </div>
          </section>

        </>
      ) : (
        <EmptyState
          icon={<ImageIcon size={20} />}
          title="还没有照片"
          description="打开一张照片开始编辑。原图只读，不会被这次编辑覆盖。"
          action={
            <Button variant="primary" size="sm" icon={<FolderOpen size={14} />} onClick={() => void openPhoto()}>
              打开照片
            </Button>
          }
        />
      )}
    </div>
  );
}
