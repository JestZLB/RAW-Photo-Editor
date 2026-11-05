export const MAX_IMPORT_BYTES = 32 * 1024 * 1024;
export const ACCEPTED_IMAGE_EXTENSIONS = ["jpg", "jpeg", "png", "tif", "tiff", "webp"] as const;
export const IMAGE_ACCEPT_ATTR =
  ".jpg,.jpeg,.png,.tif,.tiff,.webp,image/jpeg,image/png,image/tiff,image/webp";
export const CUBE_ACCEPT_ATTR = ".cube,text/plain,application/octet-stream";
export const MAX_LUT_BYTES = 8 * 1024 * 1024;

export function extensionOf(fileName: string): string {
  const dot = fileName.lastIndexOf(".");
  if (dot < 0) return "";
  return fileName.slice(dot + 1).toLowerCase();
}

export function mimeForFileName(fileName: string): string {
  switch (extensionOf(fileName)) {
    case "jpg":
    case "jpeg":
      return "image/jpeg";
    case "png":
      return "image/png";
    case "tif":
    case "tiff":
      return "image/tiff";
    case "webp":
      return "image/webp";
    default:
      return "application/octet-stream";
  }
}

export function isSupportedImageFile(fileName: string, mime?: string): boolean {
  const ext = extensionOf(fileName);
  if ((ACCEPTED_IMAGE_EXTENSIONS as readonly string[]).includes(ext)) return true;
  return typeof mime === "string" && /^image\/(jpeg|png|tiff|webp)$/i.test(mime);
}

export function readAsDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("FILE_READ_FAILED"));
    reader.onload = () => {
      if (typeof reader.result === "string") resolve(reader.result);
      else reject(new Error("FILE_READ_FAILED"));
    };
    reader.readAsDataURL(blob);
  });
}

export async function blobToBase64(blob: Blob): Promise<string> {
  const dataUrl = await readAsDataUrl(blob);
  const comma = dataUrl.indexOf(",");
  return comma >= 0 ? dataUrl.slice(comma + 1) : dataUrl;
}

export function base64ToBlob(base64: string, mime: string): Blob {
  const clean = base64.includes(",") ? base64.slice(base64.indexOf(",") + 1) : base64;
  const binary = atob(clean);
  const chunkSize = 8192;
  const chunks: BlobPart[] = [];
  for (let offset = 0; offset < binary.length; offset += chunkSize) {
    const end = Math.min(binary.length, offset + chunkSize);
    const chunk = new Uint8Array(end - offset);
    for (let index = offset; index < end; index += 1) {
      chunk[index - offset] = binary.charCodeAt(index);
    }
    chunks.push(chunk);
  }
  return new Blob(chunks, { type: mime });
}

/** Opens the OS file picker in browser demo mode via a transient hidden input. */
export function pickFiles(accept: string, multiple = false): Promise<File[]> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.multiple = multiple;
    input.style.position = "fixed";
    input.style.left = "-9999px";
    input.style.opacity = "0";
    document.body.appendChild(input);

    let settled = false;
    const finish = (files: File[]): void => {
      if (settled) return;
      settled = true;
      input.remove();
      window.removeEventListener("focus", onFocus);
      resolve(files);
    };
    const onChange = (): void => {
      finish(input.files ? Array.from(input.files) : []);
    };
    const onFocus = (): void => {
      window.setTimeout(() => {
        if (!settled && (!input.files || input.files.length === 0)) finish([]);
      }, 400);
    };

    input.addEventListener("change", onChange);
    input.addEventListener("cancel", () => finish([]));
    window.addEventListener("focus", onFocus, { once: true });
    input.click();
  });
}

export function pickFile(accept: string): Promise<File | null> {
  return pickFiles(accept).then((files) => files[0] ?? null);
}

export function pickImageFile(): Promise<File | null> {
  return pickFile(IMAGE_ACCEPT_ATTR);
}

/** Multi-select photo picker used by the browser demo's "add photos" flow. */
export function pickImageFiles(): Promise<File[]> {
  return pickFiles(IMAGE_ACCEPT_ATTR, true);
}

export function downloadDataUrl(dataUrl: string, fileName: string): void {
  const anchor = document.createElement("a");
  anchor.href = dataUrl;
  anchor.download = fileName;
  anchor.rel = "noopener";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
}
