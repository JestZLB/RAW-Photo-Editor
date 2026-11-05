import { useEffect } from "react";
import { Eye, EyeOff, ScanFace } from "lucide-react";
import { resolveAssetUrl } from "../../api/assets";
import type { PortraitFace } from "../../api/types";
import { cn } from "../../lib/cn";
import { faceQuality, faceScorePercent } from "../../lib/face";
import { useEditorStore } from "../../stores/editorStore";
import { IconButton } from "../../components/controls/Button";
import { Spinner } from "../../components/controls/Spinner";

function FaceThumb({
  face,
  index,
  sourceWidth,
  sourceHeight,
  imageUrl,
}: {
  face: PortraitFace;
  index: number;
  sourceWidth: number;
  sourceHeight: number;
  imageUrl: string | null;
}) {
  if (!imageUrl || sourceWidth <= 0 || sourceHeight <= 0) {
    return <span className="face-item__index">{index + 1}</span>;
  }
  const side = Math.max(face.width, face.height) * 1.75;
  const cropWidth = Math.min(1, side / sourceWidth);
  const cropHeight = Math.min(1, side / sourceHeight);
  const centerX = (face.x + face.width / 2) / sourceWidth;
  const centerY = (face.y + face.height / 2) / sourceHeight;
  const left = Math.max(0, Math.min(1 - cropWidth, centerX - cropWidth / 2));
  const top = Math.max(0, Math.min(1 - cropHeight, centerY - cropHeight / 2));
  return (
    <span className="face-thumb">
      <img
        src={imageUrl}
        alt=""
        style={{
          width: `${100 / cropWidth}%`,
          height: `${100 / cropHeight}%`,
          left: `${(-left * 100) / cropWidth}%`,
          top: `${(-top * 100) / cropHeight}%`,
        }}
      />
    </span>
  );
}

/**
 * Compact always-visible face selector. Detection runs automatically; the
 * thumbnails stay on one row so the beauty controls begin above the fold.
 */
export function FacePicker() {
  const api = useEditorStore((state) => state.api);
  const mode = useEditorStore((state) => state.mode);
  const doc = useEditorStore((state) => state.doc);
  const faceAnalysis = useEditorStore((state) => state.faceAnalysis);
  const faces = faceAnalysis?.faces ?? [];
  const faceBusy = useEditorStore((state) => state.faceBusy);
  const faceError = useEditorStore((state) => state.faceError);
  const faceAnalysisDocId = useEditorStore((state) => state.faceAnalysisDocId);
  const selectedFaceIndex = useEditorStore((state) => state.selectedFaceIndex);
  const ignoredFaceIds = useEditorStore((state) => state.ignoredFaceIds);
  const setSelectedFaceIndex = useEditorStore((state) => state.setSelectedFaceIndex);
  const toggleFaceIgnored = useEditorStore((state) => state.toggleFaceIgnored);
  const detectFaces = useEditorStore((state) => state.detectFaces);

  useEffect(() => {
    if (mode === "tauri" && doc?.source && faceAnalysisDocId !== doc.id && !faceBusy) {
      void detectFaces();
    }
  }, [mode, doc?.id, doc?.source, faceAnalysisDocId, faceBusy, detectFaces]);

  const faceLayers = new Set(
    (doc?.layers ?? [])
      .filter((item) => item.kind === "portrait" && item.portrait?.faceIndex != null)
      .map((item) => item.portrait?.faceIndex as number),
  );
  const source = doc?.source ?? null;
  const imageUrl = resolveAssetUrl(source?.previewPath);
  const ignoredCount = faces.filter((face) => ignoredFaceIds.includes(face.faceId)).length;

  return (
    <div className="face-bar">
      <div className="face-bar__head">
        <span className="face-bar__title">人脸</span>
        <span className="face-bar__hint" title="编号按画面从左到右；分数用于相对比较，侧脸、遮挡或浓妆会偏低">
          {faceBusy
            ? "识别中…"
            : faces.length > 1
              ? `${faces.length - ignoredCount} 张可调整${
                  ignoredCount > 0 ? ` · ${ignoredCount} 张已忽略` : ""
                }`
              : faces.length === 1
                ? "1 张 · 已自动识别"
                : "自动识别"}
        </span>
        <IconButton
          label={faceAnalysis ? "重新检测人脸" : "检测人脸"}
          size="sm"
          icon={faceBusy ? <Spinner size={12} /> : <ScanFace size={14} />}
          disabled={!api || mode !== "tauri" || faceBusy}
          onClick={() => void detectFaces()}
        />
      </div>
      {faceError ? <p className="face-detect__error">{faceError}</p> : null}
      {faceAnalysis && !faceAnalysis.available ? (
        <p className="face-detect__error">
          {faceAnalysis.warnings[0]?.message ?? "人脸模型暂时不可用，请稍后重试。"}
        </p>
      ) : null}
      {faces.length > 0 ? (
        <div className="face-bar__list" role="group" aria-label="检测到的人脸">
          {faces.map((face, index) => {
            const percent = faceScorePercent(face.confidence);
            const quality = faceQuality(face.confidence);
            const ignored = ignoredFaceIds.includes(face.faceId);
            const selected = index === selectedFaceIndex && !ignored;
            return (
              <div
                key={face.faceId}
                className={cn("face-chip", selected && "is-active", ignored && "is-ignored")}
              >
                <button
                  type="button"
                  className="face-chip__select"
                  aria-pressed={selected}
                  title={`人脸 ${index + 1} · 置信度 ${percent}%（${quality}）· ${
                    face.models && face.models.length > 1
                      ? "CenterFace + YuNet 双模型"
                      : face.models?.[0] === "yunet"
                        ? "YuNet"
                        : "CenterFace"
                  } · ${Math.round(face.width)} × ${Math.round(face.height)} px${
                    ignored ? " · 已忽略" : ""
                  }`}
                  onClick={() => {
                    if (ignored) toggleFaceIgnored(face.faceId);
                    else setSelectedFaceIndex(index);
                  }}
                >
                  <FaceThumb
                    face={face}
                    index={index}
                    sourceWidth={source?.width ?? 0}
                    sourceHeight={source?.height ?? 0}
                    imageUrl={imageUrl}
                  />
                  <span className="face-chip__label">人脸 {index + 1}</span>
                  <span className="face-chip__score">{ignored ? "已忽略" : `${percent}%`}</span>
                  {faceLayers.has(index) ? (
                    <span className="face-chip__dot" title="已有独立美颜层" />
                  ) : null}
                </button>
                <button
                  type="button"
                  className="face-chip__ignore"
                  aria-label={ignored ? `恢复人脸 ${index + 1}` : `忽略人脸 ${index + 1}`}
                  title={ignored ? "恢复到可调整列表" : "忽略这张脸，不参与选择与叠加显示"}
                  onClick={() => toggleFaceIgnored(face.faceId)}
                >
                  {ignored ? <Eye size={11} /> : <EyeOff size={11} />}
                </button>
              </div>
            );
          })}
        </div>
      ) : faceAnalysis && faceAnalysis.available && !faceBusy ? (
        <p className="inline-note">
          没有检测到清晰人脸，换一张光线更均匀的照片再试。
        </p>
      ) : null}
    </div>
  );
}