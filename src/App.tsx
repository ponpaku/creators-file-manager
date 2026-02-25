import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import {
  cancelOperation,
  compressCollectInfo,
  compressEstimate,
  executeCompress,
  executeDelete,
  executeExifOffset,
  executeFlatten,
  executeMetadataStrip,
  executeRename,
  exportSettings,
  getSettingsPath,
  importSettings,
  isDirectoryPath,
  isFfprobeAvailable,
  listRenameTemplateTags,
  loadSettings,
  openSettingsFolder,
  previewCompress,
  previewDelete,
  previewExifOffset,
  previewFlatten,
  previewImportConflicts,
  previewMetadataStrip,
  previewRename,
  saveSettings
} from "./api";
import type {
  AppSettings,
  CompressCollectInfoResponse,
  CompressEstimateResponse,
  CompressExecuteResponse,
  CompressPreviewResponse,
  DeleteExecuteResponse,
  DeletePattern,
  DeletePreviewResponse,
  EstimateProgressEvent,
  ExifOffsetExecuteResponse,
  ExifOffsetPreviewResponse,
  FlattenExecuteResponse,
  FlattenPreviewResponse,
  ImportConflictPreview,
  MetadataStripCategories,
  MetadataStripExecuteResponse,
  MetadataStripPreviewResponse,
  MetadataStripPreset,
  OperationProgressEvent,
  RenameExecuteResponse,
  RenamePreviewResponse,
  RenameSource,
  RenameTemplateTag
} from "./types";

type TabKey = "rename" | "delete" | "compress" | "flatten" | "exif-offset" | "metadata-strip" | "settings" | "about";

type Toast = {
  id: number;
  type: "success" | "error" | "info";
  message: string;
};

const NAV_ITEMS: { key: TabKey; label: string; desc: string }[] = [
  { key: "rename", label: "一括リネーム", desc: "動画・画像ファイルを撮影日時やテンプレートで一括リネーム" },
  { key: "delete", label: "拡張子一括削除", desc: "指定した拡張子のファイルを一括で削除・退避" },
  { key: "compress", label: "JPEG一括圧縮", desc: "JPEGファイルのリサイズ・品質調整を一括で実行" },
  { key: "exif-offset", label: "EXIF日時補正", desc: "JPEGのEXIF撮影日時をオフセット補正" },
  { key: "metadata-strip", label: "メタデータ削除", desc: "JPEGのEXIFから指定情報を削除" },
  { key: "flatten", label: "フォルダ展開", desc: "フォルダ構造を展開し、すべてのファイルをフラットにコピー" },
  { key: "settings", label: "設定", desc: "アプリケーションの設定を管理" },
  { key: "about", label: "このアプリについて", desc: "アプリケーション情報" }
];

const NavIcon = ({ tabKey }: { tabKey: TabKey }) => {
  switch (tabKey) {
    case "rename":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7" />
          <path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z" />
        </svg>
      );
    case "delete":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="3 6 5 6 21 6" />
          <path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2" />
        </svg>
      );
    case "compress":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
          <circle cx="8.5" cy="8.5" r="1.5" />
          <polyline points="21 15 16 10 5 21" />
        </svg>
      );
    case "flatten":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" />
        </svg>
      );
    case "exif-offset":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="12" cy="12" r="10" />
          <polyline points="12 6 12 12 16 14" />
        </svg>
      );
    case "metadata-strip":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
        </svg>
      );
    case "settings":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 010-2.83 2 2 0 012.83 0l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 0 2 2 0 010 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z" />
        </svg>
      );
    case "about":
      return (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="12" cy="12" r="10" />
          <line x1="12" y1="16" x2="12" y2="12" />
          <line x1="12" y1="8" x2="12.01" y2="8" />
        </svg>
      );
  }
};

const DropIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4" />
    <polyline points="7 10 12 15 17 10" />
    <line x1="12" y1="15" x2="12" y2="3" />
  </svg>
);

const DEFAULT_SETTINGS: AppSettings = {
  deletePatterns: [],
  renameTemplates: [{ name: "日付通番", template: "{capture_date:YYYYMMDD}_{capture_time:HHmmss}_{seq:3}" }],
  outputDirectories: {},
  theme: "system"
};

const WINDOW_STATE_KEY = "creators-file-manager.window-state.v1";

type WindowState = {
  x: number;
  y: number;
  width: number;
  height: number;
};

const parsePaths = (value: string): string[] =>
  value
    .split(/\r?\n|,/g)
    .map((item) => item.trim())
    .filter(Boolean);

const parseExts = (value: string): string[] =>
  value
    .split(/[\r\n,\s]+/g)
    .map((item) => item.trim().replace(/^\./, "").toLowerCase())
    .filter(Boolean);

const parsePositiveInt = (value: string): number | null => {
  const parsed = Number.parseInt(value.trim(), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const formatBytes = (value: number): string => {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 * 1024 * 1024) return `${(value / 1024 / 1024).toFixed(2)} MB`;
  return `${(value / 1024 / 1024 / 1024).toFixed(2)} GB`;
};

const UNIT_TO_KB: Record<string, number> = { GB: 1024 * 1024, MB: 1024, KB: 1 };

const tabToOperation = (tabKey: string): string => {
  if (tabKey === "exif-offset") return "exifOffset";
  if (tabKey === "metadata-strip") return "metadataStrip";
  return tabKey;
};

const operationLabel = (value: OperationProgressEvent["operation"]): string => {
  switch (value) {
    case "rename":
      return "リネーム";
    case "delete":
      return "削除";
    case "compress":
      return "圧縮";
    case "flatten":
      return "展開";
    case "exifOffset":
      return "EXIF日時補正";
    case "metadataStrip":
      return "メタデータ削除";
    default:
      return value;
  }
};

const StatusBadge = ({ status }: { status: string }) => {
  switch (status) {
    case "ready":
      return <span className="badge badge-ready">実行可</span>;
    case "skipped":
      return <span className="badge badge-skip">スキップ</span>;
    case "succeeded":
      return <span className="badge badge-success">成功</span>;
    case "failed":
      return <span className="badge badge-error">失敗</span>;
    default:
      return <span className="badge">{status}</span>;
  }
};

const deleteActionLabel = (value: string): string => {
  switch (value) {
    case "direct":
      return "直接削除";
    case "trash":
      return "ゴミ箱移動";
    case "retreat":
      return "退避";
    default:
      return value;
  }
};

const hasImportConflicts = (preview: ImportConflictPreview | null): boolean => {
  if (!preview) return false;
  return (
    preview.deletePatternNames.length > 0 ||
    preview.renameTemplateNames.length > 0 ||
    preview.outputDirectoryKeys.length > 0 ||
    preview.themeConflict
  );
};

const ToastItem = ({ toast, onDismiss }: { toast: Toast; onDismiss: (id: number) => void }) => {
  const [exiting, setExiting] = useState(false);
  const timerRef = useRef<number | null>(null);
  const pausedRef = useRef(false);

  const startDismiss = useCallback(() => {
    setExiting(true);
    setTimeout(() => onDismiss(toast.id), 200);
  }, [toast.id, onDismiss]);

  const startTimer = useCallback(() => {
    if (timerRef.current !== null) window.clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(() => {
      if (!pausedRef.current) startDismiss();
    }, 5000);
  }, [startDismiss]);

  useEffect(() => {
    startTimer();
    return () => {
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
    };
  }, [startTimer]);

  return (
    <div
      className={`toast-item toast-${toast.type}${exiting ? " toast-exit" : ""}`}
      onMouseEnter={() => {
        pausedRef.current = true;
        if (timerRef.current !== null) window.clearTimeout(timerRef.current);
      }}
      onMouseLeave={() => {
        pausedRef.current = false;
        startTimer();
      }}
    >
      <span className="toast-message">{toast.message}</span>
      <button className="toast-dismiss" type="button" onClick={startDismiss}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
      </button>
    </div>
  );
};

const mergePathText = (current: string, incoming: string[]): string => {
  const merged = Array.from(new Set([...parsePaths(current), ...incoming.filter(Boolean)]));
  return merged.join("\n");
};

const parseWindowState = (raw: string | null): WindowState | null => {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as Partial<WindowState>;
    if (
      typeof parsed.x !== "number" ||
      typeof parsed.y !== "number" ||
      typeof parsed.width !== "number" ||
      typeof parsed.height !== "number" ||
      parsed.width < 200 ||
      parsed.height < 200
    ) {
      return null;
    }
    return parsed as WindowState;
  } catch {
    return null;
  }
};

export function App() {
  const [tab, setTab] = useState<TabKey>("rename");
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [settingsPath, setSettingsPath] = useState("");
  const [settingsStatus, setSettingsStatus] = useState("");
  const [isBusy, setIsBusy] = useState(false);
  const [progressMap, setProgressMap] = useState<Record<string, OperationProgressEvent>>({});
  const [isDragOver, setIsDragOver] = useState(false);

  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastIdRef = useRef(0);

  const [renamePaths, setRenamePaths] = useState("");
  const [renameSubfolders, setRenameSubfolders] = useState(false);
  const [renameTemplate, setRenameTemplate] = useState(DEFAULT_SETTINGS.renameTemplates[0].template);
  const [renameSource, setRenameSource] = useState<RenameSource>("captureThenModified");
  const [renameOutputDir, setRenameOutputDir] = useState("");
  const [renameConflictPolicy, setRenameConflictPolicy] = useState<"overwrite" | "sequence" | "skip">("sequence");
  const [ffprobeAvailable, setFfprobeAvailable] = useState(false);
  const [useFfprobe, setUseFfprobe] = useState(false);
  const [renamePreview, setRenamePreview] = useState<RenamePreviewResponse | null>(null);
  const [renameExec, setRenameExec] = useState<RenameExecuteResponse | null>(null);
  const [renameTemplateTags, setRenameTemplateTags] = useState<RenameTemplateTag[]>([]);
  const [templateSelected, setTemplateSelected] = useState("");
  const [templateOriginal, setTemplateOriginal] = useState<{ name: string; template: string } | null>(null);
  const [isCreatingNewTemplate, setIsCreatingNewTemplate] = useState(false);
  const [newTemplateName, setNewTemplateName] = useState("");

  const [deletePaths, setDeletePaths] = useState("");
  const [deleteSubfolders, setDeleteSubfolders] = useState(false);
  const [deleteExtensions, setDeleteExtensions] = useState("jpg");
  const [deleteMode, setDeleteMode] = useState<DeletePattern["mode"]>("trash");
  const [deleteRetreatDir, setDeleteRetreatDir] = useState("");
  const [deleteConflictPolicy, setDeleteConflictPolicy] = useState<"overwrite" | "sequence" | "skip">("sequence");
  const [deletePreview, setDeletePreview] = useState<DeletePreviewResponse | null>(null);
  const [deleteExec, setDeleteExec] = useState<DeleteExecuteResponse | null>(null);
  const [patternSelected, setPatternSelected] = useState("");
  const [patternOriginal, setPatternOriginal] = useState<DeletePattern | null>(null);
  const [isCreatingNewPattern, setIsCreatingNewPattern] = useState(false);
  const [newPatternName, setNewPatternName] = useState("");

  const [compressPaths, setCompressPaths] = useState("");
  const [compressSubfolders, setCompressSubfolders] = useState(false);
  const [compressResizePercent, setCompressResizePercent] = useState(100);
  const [compressQuality, setCompressQuality] = useState(82);
  const [compressTargetSizeKb, setCompressTargetSizeKb] = useState("");
  const [compressTargetSizeUnit, setCompressTargetSizeUnit] = useState<"GB" | "MB" | "KB">("MB");
  const [compressTolerancePercent, setCompressTolerancePercent] = useState(10);
  const [compressPreserveExif, setCompressPreserveExif] = useState(true);
  const [compressOutputDir, setCompressOutputDir] = useState("");
  const [compressConflictPolicy, setCompressConflictPolicy] = useState<"overwrite" | "sequence" | "skip">("sequence");
  const [compressSourceInfo, setCompressSourceInfo] = useState<CompressCollectInfoResponse | null>(null);
  const [compressEstimateResult, setCompressEstimateResult] = useState<CompressEstimateResponse | null>(null);
  const [estimateProgress, setEstimateProgress] = useState<EstimateProgressEvent | null>(null);
  const [compressPreview, setCompressPreview] = useState<CompressPreviewResponse | null>(null);
  const [compressExec, setCompressExec] = useState<CompressExecuteResponse | null>(null);
  const [busyLabel, setBusyLabel] = useState<string | null>(null);

  const [flattenInputDir, setFlattenInputDir] = useState("");
  const [flattenOutputDir, setFlattenOutputDir] = useState("");
  const [flattenConflictPolicy, setFlattenConflictPolicy] = useState<"overwrite" | "sequence" | "skip">("sequence");
  const [flattenPreview, setFlattenPreview] = useState<FlattenPreviewResponse | null>(null);
  const [flattenExec, setFlattenExec] = useState<FlattenExecuteResponse | null>(null);

  const [exifOffsetPaths, setExifOffsetPaths] = useState("");
  const [exifOffsetSubfolders, setExifOffsetSubfolders] = useState(false);
  const [exifOffsetSign, setExifOffsetSign] = useState<"+" | "-">("+");
  const [exifOffsetDays, setExifOffsetDays] = useState(0);
  const [exifOffsetHours, setExifOffsetHours] = useState(0);
  const [exifOffsetMinutes, setExifOffsetMinutes] = useState(0);
  const [exifOffsetSeconds, setExifOffsetSeconds] = useState(0);
  const [exifOffsetPreview, setExifOffsetPreview] = useState<ExifOffsetPreviewResponse | null>(null);
  const [exifOffsetExec, setExifOffsetExec] = useState<ExifOffsetExecuteResponse | null>(null);

  const SNS_CATEGORIES: MetadataStripCategories = { gps: true, cameraLens: true, software: false, authorCopyright: false, comments: true, thumbnail: true, iptc: false, xmp: false, shootingSettings: false, captureDateTime: false };
  const [metadataStripPaths, setMetadataStripPaths] = useState("");
  const [metadataStripSubfolders, setMetadataStripSubfolders] = useState(false);
  const [metadataStripPreset, setMetadataStripPreset] = useState<MetadataStripPreset>("snsPublish");
  const [metadataStripCategories, setMetadataStripCategories] = useState<MetadataStripCategories>(SNS_CATEGORIES);
  const [metadataStripPreview, setMetadataStripPreview] = useState<MetadataStripPreviewResponse | null>(null);
  const [metadataStripExec, setMetadataStripExec] = useState<MetadataStripExecuteResponse | null>(null);

  const [exportPath, setExportPath] = useState("");
  const [importPath, setImportPath] = useState("");
  const [importMode, setImportMode] = useState<"overwrite" | "merge">("merge");
  const [importConflictPolicy, setImportConflictPolicy] = useState<"existing" | "import" | "cancel">("existing");
  const [importConflictPreview, setImportConflictPreview] = useState<ImportConflictPreview | null>(null);

  const renameTemplateRef = useRef<HTMLInputElement>(null);
  const estimateSeqRef = useRef(0);

  const tabRef = useRef<TabKey>(tab);
  const windowSaveTimerRef = useRef<number | null>(null);
  const outputSaveTimerRef = useRef<number | null>(null);
  const settingsRef = useRef<AppSettings>(DEFAULT_SETTINGS);
  const settingsLoadedRef = useRef(false);

  const renameFiles = useMemo(() => parsePaths(renamePaths), [renamePaths]);
  const deleteFiles = useMemo(() => parsePaths(deletePaths), [deletePaths]);
  const compressFiles = useMemo(() => parsePaths(compressPaths), [compressPaths]);
  const exifOffsetFiles = useMemo(() => parsePaths(exifOffsetPaths), [exifOffsetPaths]);
  const metadataStripFiles = useMemo(() => parsePaths(metadataStripPaths), [metadataStripPaths]);
  const totalOffsetSeconds = useMemo(() => {
    const abs = exifOffsetDays * 86400 + exifOffsetHours * 3600 + exifOffsetMinutes * 60 + exifOffsetSeconds;
    return exifOffsetSign === "+" ? abs : -abs;
  }, [exifOffsetSign, exifOffsetDays, exifOffsetHours, exifOffsetMinutes, exifOffsetSeconds]);
  const patterns = useMemo(
    () => [...settings.deletePatterns].sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase())),
    [settings.deletePatterns]
  );
  const templates = useMemo(
    () => [...settings.renameTemplates].sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase())),
    [settings.renameTemplates]
  );
  const templateModified = useMemo(() => {
    if (!templateOriginal) return false;
    return renameTemplate !== templateOriginal.template;
  }, [templateOriginal, renameTemplate]);
  const patternModified = useMemo(() => {
    if (!patternOriginal) return false;
    const currentExts = parseExts(deleteExtensions);
    const origExts = patternOriginal.extensions;
    if (currentExts.length !== origExts.length || currentExts.some((e, i) => e !== origExts[i])) return true;
    if (deleteMode !== patternOriginal.mode) return true;
    if (deleteMode === "retreat" && deleteRetreatDir.trim() !== (patternOriginal.retreatDir ?? "")) return true;
    return false;
  }, [patternOriginal, deleteExtensions, deleteMode, deleteRetreatDir]);

  const compressSizeSummary = useMemo(() => {
    if (!compressSourceInfo) return null;
    const totalSource = compressSourceInfo.totalSize;
    const totalEstimated = compressEstimateResult
      ? compressEstimateResult.estimatedTotalSize
      : (() => {
          const resizeRatio = compressResizePercent / 100;
          const qualityFactor = Math.pow(compressQuality / 100, 1.25);
          return totalSource * resizeRatio * resizeRatio * qualityFactor;
        })();
    const parsed = parsePositiveInt(compressTargetSizeKb);
    const targetBytes = parsed !== null ? parsed * UNIT_TO_KB[compressTargetSizeUnit] * 1024 : null;
    const isSampled = compressEstimateResult !== null;
    return { totalSource, totalEstimated, targetBytes, isSampled };
  }, [compressSourceInfo, compressEstimateResult, compressResizePercent, compressQuality, compressTargetSizeKb, compressTargetSizeUnit]);

  const currentNav = NAV_ITEMS.find((item) => item.key === tab)!;

  const addToast = useCallback((type: Toast["type"], message: string) => {
    const id = ++toastIdRef.current;
    setToasts((prev) => [...prev, { id, type, message }]);
  }, []);

  const removeToast = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const insertTemplateTag = (tag: string) => {
    const input = renameTemplateRef.current;
    if (!input) {
      setRenameTemplate((prev) => prev + tag);
      return;
    }
    const start = input.selectionStart ?? renameTemplate.length;
    const end = input.selectionEnd ?? start;
    const next = renameTemplate.slice(0, start) + tag + renameTemplate.slice(end);
    setRenameTemplate(next);
    requestAnimationFrame(() => {
      const cursor = start + tag.length;
      input.setSelectionRange(cursor, cursor);
      input.focus();
    });
  };

  useEffect(() => {
    tabRef.current = tab;
  }, [tab]);

  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);

  useEffect(() => {
    void (async () => {
      try {
        const [loadedSettings, loadedPath, ffprobe, templateTags] = await Promise.all([
          loadSettings(),
          getSettingsPath(),
          isFfprobeAvailable(),
          listRenameTemplateTags().catch(() => [])
        ]);
        setSettings(loadedSettings);
        setSettingsPath(loadedPath);
        setFfprobeAvailable(ffprobe);
        setUseFfprobe(ffprobe);
        setRenameTemplateTags(templateTags);
        if (loadedSettings.renameTemplates[0]) {
          setRenameTemplate(loadedSettings.renameTemplates[0].template);
          setTemplateSelected(loadedSettings.renameTemplates[0].name);
          setTemplateOriginal(loadedSettings.renameTemplates[0]);
        }
        setRenameOutputDir(loadedSettings.outputDirectories.rename ?? "");
        setDeleteRetreatDir(loadedSettings.outputDirectories.delete_retreat ?? "");
        setCompressOutputDir(loadedSettings.outputDirectories.compress ?? "");
        setFlattenOutputDir(loadedSettings.outputDirectories.flatten ?? "");
        settingsLoadedRef.current = true;
      } catch (loadError) {
        addToast("error", String(loadError));
      }
    })();

    let unlistenProgress: (() => void) | null = null;
    void listen<OperationProgressEvent>("operation-progress", (event) => {
      const p = event.payload;
      setProgressMap((prev) => ({ ...prev, [p.operation]: p }));
    }).then((fn) => {
      unlistenProgress = fn;
    });

    let unlistenEstimate: (() => void) | null = null;
    void listen<EstimateProgressEvent>("compress-estimate-progress", (event) => {
      setEstimateProgress(event.payload);
    }).then((fn) => {
      unlistenEstimate = fn;
    });

    return () => {
      if (unlistenProgress) unlistenProgress();
      if (unlistenEstimate) unlistenEstimate();
    };
  }, []);

  useEffect(() => {
    const theme = settings.theme;
    if (theme === "system") {
      document.documentElement.removeAttribute("data-theme");
    } else {
      document.documentElement.setAttribute("data-theme", theme);
    }
  }, [settings.theme]);

  useEffect(() => {
    if (!settingsLoadedRef.current) return;
    if (outputSaveTimerRef.current !== null) {
      window.clearTimeout(outputSaveTimerRef.current);
    }
    outputSaveTimerRef.current = window.setTimeout(() => {
      const next: AppSettings = {
        ...settingsRef.current,
        outputDirectories: {
          ...settingsRef.current.outputDirectories,
          rename: renameOutputDir.trim(),
          delete_retreat: deleteRetreatDir.trim(),
          compress: compressOutputDir.trim(),
          flatten: flattenOutputDir.trim()
        }
      };
      settingsRef.current = next;
      setSettings(next);
      void saveSettings(next).catch((saveError) => {
        addToast("error", String(saveError));
      });
    }, 400);
    return () => {
      if (outputSaveTimerRef.current !== null) {
        window.clearTimeout(outputSaveTimerRef.current);
      }
    };
  }, [renameOutputDir, deleteRetreatDir, compressOutputDir, flattenOutputDir]);

  useEffect(() => {
    const win = getCurrentWindow();
    const restored = parseWindowState(localStorage.getItem(WINDOW_STATE_KEY));
    if (restored) {
      void win.setSize(new LogicalSize(restored.width, restored.height));
      void win.setPosition(new LogicalPosition(restored.x, restored.y));
    }

    const saveWindowState = async () => {
      try {
        const [size, position] = await Promise.all([win.outerSize(), win.outerPosition()]);
        const snapshot: WindowState = {
          x: position.x,
          y: position.y,
          width: size.width,
          height: size.height
        };
        localStorage.setItem(WINDOW_STATE_KEY, JSON.stringify(snapshot));
      } catch {
      }
    };

    const scheduleSave = () => {
      if (windowSaveTimerRef.current !== null) {
        window.clearTimeout(windowSaveTimerRef.current);
      }
      windowSaveTimerRef.current = window.setTimeout(() => {
        void saveWindowState();
      }, 150);
    };

    let unlistenResize: (() => void) | null = null;
    let unlistenMove: (() => void) | null = null;
    void win.onResized(() => {
      scheduleSave();
    }).then((fn) => {
      unlistenResize = fn;
    });
    void win.onMoved(() => {
      scheduleSave();
    }).then((fn) => {
      unlistenMove = fn;
    });

    return () => {
      if (windowSaveTimerRef.current !== null) {
        window.clearTimeout(windowSaveTimerRef.current);
      }
      if (unlistenResize) unlistenResize();
      if (unlistenMove) unlistenMove();
    };
  }, []);

  useEffect(() => {
    if (!compressFiles.length) {
      setCompressSourceInfo(null);
      return;
    }
    const timer = window.setTimeout(() => {
      void compressCollectInfo(compressFiles, compressSubfolders)
        .then(setCompressSourceInfo)
        .catch(() => setCompressSourceInfo(null));
    }, 300);
    return () => window.clearTimeout(timer);
  }, [compressFiles, compressSubfolders]);

  useEffect(() => {
    if (!compressFiles.length) {
      setCompressEstimateResult(null);
      setEstimateProgress(null);
      return;
    }
    setCompressEstimateResult(null);
    setEstimateProgress(null);
    const seq = ++estimateSeqRef.current;
    const timer = window.setTimeout(() => {
      void compressEstimate(compressFiles, compressSubfolders, compressResizePercent, compressQuality)
        .then((result) => {
          if (estimateSeqRef.current === seq) {
            setCompressEstimateResult(result);
            setEstimateProgress(null);
          }
        })
        .catch(() => {
          if (estimateSeqRef.current === seq) {
            setCompressEstimateResult(null);
            setEstimateProgress(null);
          }
        });
    }, 500);
    return () => window.clearTimeout(timer);
  }, [compressFiles, compressSubfolders, compressResizePercent, compressQuality]);

  const run = async (action: () => Promise<void>, label?: string) => {
    setIsBusy(true);
    setBusyLabel(label ?? null);
    try {
      await action();
    } catch (runError) {
      addToast("error", String(runError));
    } finally {
      setIsBusy(false);
      setBusyLabel(null);
    }
  };

  const withOutputDirectories = (base: AppSettings): AppSettings => ({
    ...base,
    outputDirectories: {
      ...base.outputDirectories,
      rename: renameOutputDir.trim(),
      delete_retreat: deleteRetreatDir.trim(),
      compress: compressOutputDir.trim(),
      flatten: flattenOutputDir.trim()
    }
  });

  const applySettingsToInputs = (next: AppSettings) => {
    setSettings(next);
    setRenameOutputDir(next.outputDirectories.rename ?? "");
    setDeleteRetreatDir(next.outputDirectories.delete_retreat ?? "");
    setCompressOutputDir(next.outputDirectories.compress ?? "");
    setFlattenOutputDir(next.outputDirectories.flatten ?? "");
  };

  const applyDeletePattern = (name: string) => {
    const selected = settings.deletePatterns.find(
      (pattern) => pattern.name.toLowerCase() === name.toLowerCase()
    );
    if (!selected) {
      addToast("error", "選択した削除パターンが見つかりません。");
      return;
    }
    setDeleteExtensions(selected.extensions.join(", "));
    setDeleteMode(selected.mode);
    setDeleteRetreatDir(selected.retreatDir ?? "");
    setPatternOriginal(selected);
  };

  const applyRenameTemplate = (name: string) => {
    const selected = settings.renameTemplates.find(
      (t) => t.name.toLowerCase() === name.toLowerCase()
    );
    if (!selected) {
      addToast("error", "選択したテンプレートが見つかりません。");
      return;
    }
    setRenameTemplate(selected.template);
    setTemplateOriginal(selected);
  };

  const overwriteTemplate = () =>
    run(async () => {
      if (!templateSelected || !templateOriginal) {
        throw new Error("上書き保存するテンプレートを選択してください。");
      }
      if (!renameTemplate.trim()) throw new Error("テンプレートが空です。");
      const updated = { name: templateSelected, template: renameTemplate };
      const next = withOutputDirectories({
        ...settings,
        renameTemplates: settings.renameTemplates.map((t) =>
          t.name.toLowerCase() === templateSelected.toLowerCase() ? updated : t
        )
      });
      applySettingsToInputs(next);
      await saveSettings(next);
      setTemplateOriginal(updated);
      addToast("success", `テンプレートを上書き保存しました: ${templateSelected}`);
    });

  const saveNewTemplate = () =>
    run(async () => {
      const name = newTemplateName.trim();
      if (name.length < 1 || name.length > 40) throw new Error("テンプレート名は1〜40文字で入力してください。");
      if (settings.renameTemplates.some((t) => t.name.toLowerCase() === name.toLowerCase())) throw new Error("同名テンプレートが既に存在します。");
      if (!renameTemplate.trim()) throw new Error("テンプレートが空です。");
      const newTmpl = { name, template: renameTemplate };
      const next = withOutputDirectories({
        ...settings,
        renameTemplates: [...settings.renameTemplates, newTmpl]
      });
      applySettingsToInputs(next);
      await saveSettings(next);
      setTemplateSelected(name);
      setTemplateOriginal(newTmpl);
      setIsCreatingNewTemplate(false);
      setNewTemplateName("");
      addToast("success", `テンプレートを作成しました: ${name}`);
    });

  const deleteTemplateByName = () =>
    run(async () => {
      if (!templateSelected) throw new Error("削除するテンプレートを選択してください。");
      const next = withOutputDirectories({
        ...settings,
        renameTemplates: settings.renameTemplates.filter(
          (t) => t.name.toLowerCase() !== templateSelected.toLowerCase()
        )
      });
      applySettingsToInputs(next);
      await saveSettings(next);
      setTemplateSelected("");
      setTemplateOriginal(null);
      addToast("success", "テンプレートを削除しました。");
    });

  const overwritePattern = () =>
    run(async () => {
      if (!patternSelected || !patternOriginal) {
        throw new Error("上書き保存するパターンを選択してください。");
      }
      const extensions = parseExts(deleteExtensions);
      if (!extensions.length) throw new Error("削除対象の拡張子を指定してください。");
      if (deleteMode === "retreat" && !deleteRetreatDir.trim()) throw new Error("退避モードでは退避先フォルダが必須です。");

      const updated: DeletePattern = {
        name: patternSelected,
        extensions,
        mode: deleteMode,
        retreatDir: deleteMode === "retreat" ? deleteRetreatDir.trim() : null
      };
      const next = withOutputDirectories({
        ...settings,
        deletePatterns: settings.deletePatterns.map((p) =>
          p.name.toLowerCase() === patternSelected.toLowerCase() ? updated : p
        )
      });
      applySettingsToInputs(next);
      await saveSettings(next);
      setPatternOriginal(updated);
      addToast("success", `パターンを上書き保存しました: ${patternSelected}`);
    });

  const saveNewPattern = () =>
    run(async () => {
      const name = newPatternName.trim();
      if (name.length < 1 || name.length > 40) throw new Error("パターン名は1〜40文字で入力してください。");
      if (settings.deletePatterns.some((p) => p.name.toLowerCase() === name.toLowerCase())) throw new Error("同名パターンが既に存在します。");
      const extensions = parseExts(deleteExtensions);
      if (!extensions.length) throw new Error("削除対象の拡張子を指定してください。");
      if (deleteMode === "retreat" && !deleteRetreatDir.trim()) throw new Error("退避モードでは退避先フォルダが必須です。");

      const newPattern: DeletePattern = {
        name,
        extensions,
        mode: deleteMode,
        retreatDir: deleteMode === "retreat" ? deleteRetreatDir.trim() : null
      };
      const next = withOutputDirectories({
        ...settings,
        deletePatterns: [...settings.deletePatterns, newPattern]
      });
      applySettingsToInputs(next);
      await saveSettings(next);
      setPatternSelected(name);
      setPatternOriginal(newPattern);
      setIsCreatingNewPattern(false);
      setNewPatternName("");
      addToast("success", `パターンを作成しました: ${name}`);
    });

  const deletePatternByName = () =>
    run(async () => {
      if (!patternSelected) throw new Error("削除するパターンを選択してください。");
      const next = withOutputDirectories({
        ...settings,
        deletePatterns: settings.deletePatterns.filter(
          (pattern) => pattern.name.toLowerCase() !== patternSelected.toLowerCase()
        )
      });
      applySettingsToInputs(next);
      await saveSettings(next);
      setPatternSelected("");
      setPatternOriginal(null);
      addToast("success", "削除パターンを削除しました。");
    });

  const applyPathsToCurrentTab = async (incoming: string[]) => {
    const paths = incoming.filter(Boolean);
    if (!paths.length) return;

    switch (tabRef.current) {
      case "rename":
        setRenamePaths((current) => mergePathText(current, paths));
        addToast("info", `${paths.length}件のパスを追加しました`);
        break;
      case "delete":
        setDeletePaths((current) => mergePathText(current, paths));
        addToast("info", `${paths.length}件のパスを追加しました`);
        break;
      case "compress":
        setCompressPaths((current) => mergePathText(current, paths));
        addToast("info", `${paths.length}件のパスを追加しました`);
        break;
      case "flatten": {
        for (const path of paths) {
          if (await isDirectoryPath(path)) {
            setFlattenInputDir(path);
            addToast("info", "入力フォルダを設定しました");
            return;
          }
        }
        throw new Error("フォルダ展開はフォルダ入力のみ対応です。");
      }
      case "exif-offset":
        setExifOffsetPaths((current) => mergePathText(current, paths));
        addToast("info", `${paths.length}件のパスを追加しました`);
        break;
      case "metadata-strip":
        setMetadataStripPaths((current) => mergePathText(current, paths));
        addToast("info", `${paths.length}件のパスを追加しました`);
        break;
      default:
        break;
    }
  };

  const openFilesDialog = () =>
    run(async () => {
      if (tab === "flatten") {
        throw new Error("フォルダ展開はファイル選択に対応していません。");
      }
      const selected = await open({
        multiple: true,
        directory: false,
        title: "ファイルを選択"
      });
      const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
      await applyPathsToCurrentTab(paths);
    });

  const openFolderDialog = () =>
    run(async () => {
      const selected = await open({
        multiple: false,
        directory: true,
        title: "フォルダを選択"
      });
      const path = Array.isArray(selected) ? selected[0] : selected;
      if (!path) return;
      await applyPathsToCurrentTab([path]);
    });

  useEffect(() => {
    let unlistenDrop: (() => void) | null = null;
    void getCurrentWindow()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          setIsDragOver(true);
        } else if (payload.type === "leave") {
          setIsDragOver(false);
        } else if (payload.type === "drop" && "paths" in payload) {
          setIsDragOver(false);
          void run(async () => {
            const currentTab = tabRef.current;
            await applyPathsToCurrentTab(payload.paths);
            await previewCurrentTab(currentTab);
          });
        }
      })
      .then((fn) => {
        unlistenDrop = fn;
      });

    return () => {
      if (unlistenDrop) unlistenDrop();
    };
  }, []);

  const refreshImportConflicts = async (): Promise<ImportConflictPreview | null> => {
    if (!importPath.trim()) {
      throw new Error("インポート元JSONパスを指定してください。");
    }
    if (importMode !== "merge") {
      setImportConflictPreview(null);
      return null;
    }
    const preview = await previewImportConflicts(importPath.trim());
    setImportConflictPreview(preview);
    return preview;
  };

  const countConflictWarnings = (reasons: Array<string | null | undefined>): number => {
    const pattern = /(collision|overwrite|numeric suffix|連番|上書き)/i;
    return reasons.filter((reason) => reason && pattern.test(reason)).length;
  };

  const previewCurrentTab = async (tabKey: TabKey) => {
    switch (tabKey) {
      case "rename":
        if (!renameFiles.length) return;
        setRenameExec(null);
        setRenamePreview(
          await previewRename({
            inputPaths: renameFiles,
            includeSubfolders: renameSubfolders,
            template: renameTemplate,
            source: renameSource,
            outputDir: renameOutputDir.trim() || null,
            conflictPolicy: renameConflictPolicy,
            useFfprobe
          })
        );
        break;
      case "delete": {
        if (!deleteFiles.length) return;
        const extensions = parseExts(deleteExtensions);
        if (!extensions.length) return;
        setDeleteExec(null);
        const delResult = await previewDelete({
          inputPaths: deleteFiles,
          includeSubfolders: deleteSubfolders,
          extensions,
          mode: deleteMode,
          retreatDir: deleteMode === "retreat" ? deleteRetreatDir.trim() : null,
          conflictPolicy: deleteConflictPolicy
        });
        if (delResult.total === 0) {
          setDeletePreview(null);
          addToast("info", "削除対象のファイルが見つかりませんでした。");
        } else {
          setDeletePreview(delResult);
        }
        break;
      }
      case "compress": {
        if (!compressFiles.length) return;
        setCompressExec(null);
        const preview = await previewCompress({
          inputPaths: compressFiles,
          includeSubfolders: compressSubfolders,
          resizePercent: compressResizePercent,
          quality: compressQuality,
          targetSizeKb: null,
          tolerancePercent: compressTolerancePercent,
          preserveExif: compressPreserveExif,
          outputDir: compressOutputDir.trim() || null,
          conflictPolicy: compressConflictPolicy
        });
        setCompressPreview(preview);
        break;
      }
      case "flatten":
        if (!flattenInputDir.trim()) return;
        setFlattenExec(null);
        setFlattenPreview(
          await previewFlatten({
            inputDir: flattenInputDir.trim(),
            outputDir: flattenOutputDir.trim() || null,
            conflictPolicy: flattenConflictPolicy
          })
        );
        break;
      case "exif-offset":
        if (!exifOffsetFiles.length) return;
        setExifOffsetExec(null);
        setExifOffsetPreview(
          await previewExifOffset({
            inputPaths: exifOffsetFiles,
            includeSubfolders: exifOffsetSubfolders,
            offsetSeconds: totalOffsetSeconds
          })
        );
        break;
      case "metadata-strip":
        if (!metadataStripFiles.length) return;
        setMetadataStripExec(null);
        setMetadataStripPreview(
          await previewMetadataStrip({
            inputPaths: metadataStripFiles,
            includeSubfolders: metadataStripSubfolders,
            preset: metadataStripPreset,
            categories: metadataStripCategories
          })
        );
        break;
      default:
        break;
    }
  };

  /* ===== JSX ===== */

  return (
    <div className="app-layout">
      {/* ===== Sidebar ===== */}
      <aside className="sidebar">
        <div className="sidebar-brand">
          <img className="sidebar-brand-icon" src="/app-icon.png" alt="CF" />
          <span className="sidebar-brand-text">Creators File Manager</span>
        </div>
        <nav className="sidebar-nav">
          {NAV_ITEMS.filter((item) => item.key !== "settings" && item.key !== "about").map((item) => (
            <button
              key={item.key}
              className={`nav-item${tab === item.key ? " active" : ""}`}
              onClick={() => setTab(item.key)}
              type="button"
            >
              <span className="nav-icon"><NavIcon tabKey={item.key} /></span>
              <span>{item.label}</span>
            </button>
          ))}
        </nav>
        <nav className="sidebar-nav-bottom">
          {NAV_ITEMS.filter((item) => item.key === "settings" || item.key === "about").map((item) => (
            <button
              key={item.key}
              className={`nav-item${tab === item.key ? " active" : ""}`}
              onClick={() => setTab(item.key)}
              type="button"
            >
              <span className="nav-icon"><NavIcon tabKey={item.key} /></span>
              <span>{item.label}</span>
            </button>
          ))}
        </nav>
      </aside>

      {/* ===== Main Content ===== */}
      <main className="main-content">
        {/* Page Header */}
        <div className="page-header">
          <h1>{currentNav.label}</h1>
          <p className="page-desc">{currentNav.desc}</p>
        </div>

        {/* Progress Banner (per-tab) */}
        {progressMap[tabToOperation(tab)] ? (() => {
          const p = progressMap[tabToOperation(tab)];
          return (
            <div className="progress-banner">
              <div className="progress-header">
                <span className="progress-title">
                  {operationLabel(p.operation)}: {p.processed}/{p.total}
                </span>
                <span className="progress-header-actions">
                  {isBusy && busyLabel === "実行中..." ? (
                    <button className="btn btn-sm" type="button" onClick={() => void cancelOperation()}>
                      キャンセル
                    </button>
                  ) : null}
                  {!isBusy || p.done ? (
                    <button className="btn btn-sm" type="button" onClick={() => setProgressMap((prev) => {
                      const next = { ...prev };
                      delete next[tabToOperation(tab)];
                      return next;
                    })}>
                      閉じる
                    </button>
                  ) : null}
                </span>
              </div>
              <div className="progress-track">
                <div
                  className="progress-fill"
                  style={{ width: `${p.total ? Math.round((p.processed / p.total) * 100) : 0}%` }}
                />
              </div>
              <div className="progress-stats">
                <span>成功 {p.succeeded}</span>
                <span>失敗 {p.failed}</span>
                <span>スキップ {p.skipped}</span>
              </div>
            </div>
          );
        })() : null}

        {/* Indeterminate progress for operations without progress events */}
        {isBusy && !progressMap[tabToOperation(tab)] && busyLabel ? (
          <div className="indeterminate-progress">
            <div className="indeterminate-progress-label">{busyLabel}</div>
            <div className="indeterminate-progress-track">
              <div className="indeterminate-progress-bar" />
            </div>
          </div>
        ) : null}

        {/* ===== Rename Tab ===== */}
        {tab === "rename" ? (
          <>
            <div className="input-row">
              {/* Drop Zone */}
              <div className={`drop-zone${isDragOver ? " drag-over" : ""}`}>
                <div className="drop-zone-icon"><DropIcon /></div>
                <div className="drop-zone-text">ファイルまたはフォルダをここにドロップ</div>
                <div className="drop-zone-actions">
                  <button className="btn" type="button" onClick={() => void openFilesDialog()} disabled={isBusy}>ファイル選択</button>
                  <button className="btn" type="button" onClick={() => void openFolderDialog()} disabled={isBusy}>フォルダ選択</button>
                </div>
              </div>

              {/* Input Paths */}
              <div className="card">
                <div className="form-group">
                  <label className="form-label">入力パス {renameFiles.length > 0 ? <span className="text-muted">({renameFiles.length}件)</span> : null}</label>
                  <textarea className="paths-area" rows={3} value={renamePaths} onChange={(event) => setRenamePaths(event.target.value)} placeholder="パスを入力（改行またはカンマ区切り）" />
                </div>
              </div>
            </div>

            {/* Settings */}
            <form
              onSubmit={(event: FormEvent) => {
                event.preventDefault();
                void run(async () => {
                  if (!renameFiles.length) throw new Error("入力パスを指定してください。");
                  setRenameExec(null);
                  const result = await previewRename({
                    inputPaths: renameFiles,
                    includeSubfolders: renameSubfolders,
                    template: renameTemplate,
                    source: renameSource,
                    outputDir: renameOutputDir.trim() || null,
                    conflictPolicy: renameConflictPolicy,
                    useFfprobe
                  });
                  setRenamePreview(result);
                  addToast("success", `プレビュー完了: ${result.ready}/${result.total}件`);
                });
              }}
            >
              <div className="card">
                <h3 className="card-title">リネーム設定</h3>
                <div className="form-group">
                  <label className="form-label">テンプレート</label>
                  <div className="pattern-selector-row">
                    <select
                      value={isCreatingNewTemplate ? "__new__" : templateSelected}
                      onChange={(event) => {
                        const val = event.target.value;
                        if (val === "__new__") {
                          setIsCreatingNewTemplate(true);
                          setTemplateSelected("");
                          setTemplateOriginal(null);
                          setNewTemplateName("");
                        } else {
                          setIsCreatingNewTemplate(false);
                          setTemplateSelected(val);
                          setNewTemplateName("");
                          if (val) {
                            applyRenameTemplate(val);
                          } else {
                            setTemplateOriginal(null);
                          }
                        }
                      }}
                    >
                      <option value="">(選択しない)</option>
                      {templates.map((t) => (
                        <option key={t.name} value={t.name}>{t.name}</option>
                      ))}
                      <option value="__new__">+ 新規テンプレート作成...</option>
                    </select>
                    {templateSelected && !isCreatingNewTemplate ? (
                      <>
                        <button className="btn-icon" type="button" title="上書き保存" disabled={!templateModified} onClick={() => void overwriteTemplate()}>
                          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2z" /><polyline points="17 21 17 13 7 13 7 21" /><polyline points="7 3 7 8 15 8" /></svg>
                        </button>
                        <button className="btn-icon btn-icon-danger" type="button" title="テンプレート削除" onClick={() => void deleteTemplateByName()}>
                          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2" /></svg>
                        </button>
                      </>
                    ) : null}
                  </div>
                  {isCreatingNewTemplate ? (
                    <div className="pattern-inline-create">
                      <input value={newTemplateName} onChange={(event) => setNewTemplateName(event.target.value)} placeholder="テンプレート名を入力" />
                      <button className="btn btn-sm btn-primary" type="button" onClick={() => void saveNewTemplate()}>保存</button>
                      <button className="btn btn-sm" type="button" onClick={() => { setIsCreatingNewTemplate(false); setNewTemplateName(""); }}>キャンセル</button>
                    </div>
                  ) : null}
                  {templateModified && !isCreatingNewTemplate ? (
                    <span className="form-hint" style={{ color: "var(--warning)" }}>未保存の変更があります</span>
                  ) : null}
                  <input ref={renameTemplateRef} value={renameTemplate} onChange={(event) => setRenameTemplate(event.target.value)} />
                  <div className="tag-picker">
                    <span className="tag-picker-label">タグ:</span>
                    {renameTemplateTags.map((tag) => (
                      <button
                        key={tag.token}
                        className="tag-chip"
                        type="button"
                        title={tag.description}
                        onClick={() => insertTemplateTag(tag.token)}
                      >
                        {tag.label}
                      </button>
                    ))}
                  </div>
                  <span className="form-hint">クリックでカーソル位置にタグを挿入。必要に応じて日付タグと時刻タグを組み合わせてください（例: {"{capture_date:YYYY-MM-DD}_{capture_time:HH-mm-ss}"} / {"{exec_date:YYYY-MM-DD}_{exec_time:HH-mm-ss}"}）。</span>
                </div>
                <div className="form-row">
                  <div className="form-group">
                    <label className="form-label">日時ソース</label>
                    <select value={renameSource} onChange={(event) => setRenameSource(event.target.value as RenameSource)}>
                      <option value="captureThenModified">撮影日時（失敗時は更新日時）</option>
                      <option value="modifiedOnly">更新日時のみ</option>
                      <option value="currentTime">現在時刻</option>
                    </select>
                  </div>
                  <div className="form-group">
                    <label className="form-label">競合時の処理</label>
                    <select value={renameConflictPolicy} onChange={(event) => setRenameConflictPolicy(event.target.value as "overwrite" | "sequence" | "skip")}>
                      <option value="overwrite">上書き</option>
                      <option value="sequence">連番付与</option>
                      <option value="skip">スキップ</option>
                    </select>
                  </div>
                </div>
                <div className="form-group">
                  <label className="form-label">出力先フォルダ</label>
                  <input value={renameOutputDir} onChange={(event) => setRenameOutputDir(event.target.value)} placeholder="空欄で入力元フォルダに上書き" />
                </div>
                <div className="form-group">
                  <label className="checkbox-row">
                    <input type="checkbox" checked={useFfprobe} disabled={!ffprobeAvailable} onChange={(event) => setUseFfprobe(event.target.checked)} />
                    ffprobeで動画メタデータを使う {ffprobeAvailable ? "(利用可)" : "(未検出)"}
                  </label>
                  <label className="checkbox-row">
                    <input type="checkbox" checked={renameSubfolders} onChange={(event) => setRenameSubfolders(event.target.checked)} />
                    サブフォルダを含める
                  </label>
                </div>
              </div>

              {/* Actions */}
              <div className="page-actions">
                <button className="btn" disabled={isBusy}>プレビュー</button>
                <button
                  className="btn btn-primary"
                  type="button"
                  disabled={isBusy || !renameFiles.length}
                  onClick={() =>
                    void run(async () => {
                      if (!renameFiles.length) throw new Error("入力パスを指定してください。");
                      setProgressMap((prev) => { const next = { ...prev }; delete next.rename; return next; });
                      const result = await executeRename({
                        inputPaths: renameFiles,
                        includeSubfolders: renameSubfolders,
                        template: renameTemplate,
                        source: renameSource,
                        outputDir: renameOutputDir.trim() || null,
                        conflictPolicy: renameConflictPolicy,
                        useFfprobe
                      });
                      setRenameExec(result);
                      addToast("success", `リネーム完了: 成功${result.succeeded}件${result.failed > 0 ? ` / 失敗${result.failed}件` : ""}`);
                    })
                  }
                >
                  実行
                </button>
                {(renamePreview || renameExec) ? (
                  <button className="btn" type="button" disabled={isBusy} onClick={() => { setRenamePaths(""); setRenamePreview(null); setRenameExec(null); setProgressMap((prev) => { const next = { ...prev }; delete next.rename; return next; }); }}>クリア</button>
                ) : null}
              </div>
            </form>

            {/* Preview Results */}
            {renamePreview ? (
              <div className="result-section">
                {countConflictWarnings(renamePreview.items.map((item) => item.reason)) > 0 ? (
                  <div className="conflict-warning">
                    出力先の同名競合が検出されました。競合時の処理（上書き/連番付与/スキップ）を確認して実行してください。
                  </div>
                ) : null}
                <div className="result-summary">
                  <span className="result-summary-label">プレビュー</span>
                  <span className="badge badge-info">{renamePreview.ready}/{renamePreview.total} 件</span>
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>入力</th>
                        <th>出力</th>
                        <th style={{ width: 200 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {renamePreview.items.map((item) => (
                        <tr key={`${item.sourcePath}-${item.destinationPath ?? "none"}`}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td className="cell-path">{item.destinationPath ?? "-"}</td>
                          <td>{item.reason ?? "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}

            {/* Execute Results */}
            {renameExec ? (
              <div className="result-section">
                <div className="result-summary">
                  <span className="result-summary-label">実行結果</span>
                  <span className="badge badge-success">成功 {renameExec.succeeded}</span>
                  {renameExec.failed > 0 ? <span className="badge badge-error">失敗 {renameExec.failed}</span> : null}
                  {renameExec.skipped > 0 ? <span className="badge badge-skip">スキップ {renameExec.skipped}</span> : null}
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>入力</th>
                        <th>出力</th>
                        <th style={{ width: 200 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {renameExec.details.map((item) => (
                        <tr key={`${item.sourcePath}-${item.destinationPath ?? "none"}-${item.status}`}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td className="cell-path">{item.destinationPath ?? "-"}</td>
                          <td>{item.reason ?? "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}
          </>
        ) : null}

        {/* ===== Delete Tab ===== */}
        {tab === "delete" ? (
          <>
            <div className="input-row">
              {/* Drop Zone */}
              <div className={`drop-zone${isDragOver ? " drag-over" : ""}`}>
                <div className="drop-zone-icon"><DropIcon /></div>
                <div className="drop-zone-text">ファイルまたはフォルダをここにドロップ</div>
                <div className="drop-zone-actions">
                  <button className="btn" type="button" onClick={() => void openFilesDialog()} disabled={isBusy}>ファイル選択</button>
                  <button className="btn" type="button" onClick={() => void openFolderDialog()} disabled={isBusy}>フォルダ選択</button>
                </div>
              </div>

              {/* Input Paths */}
              <div className="card">
                <div className="form-group">
                  <label className="form-label">入力パス {deleteFiles.length > 0 ? <span className="text-muted">({deleteFiles.length}件)</span> : null}</label>
                  <textarea className="paths-area" rows={3} value={deletePaths} onChange={(event) => setDeletePaths(event.target.value)} placeholder="パスを入力（改行またはカンマ区切り）" />
                </div>
              </div>
            </div>

            {/* Delete Settings */}
            <form
              onSubmit={(event: FormEvent) => {
                event.preventDefault();
                void run(async () => {
                  if (!deleteFiles.length) throw new Error("入力パスを指定してください。");
                  const extensions = parseExts(deleteExtensions);
                  if (!extensions.length) throw new Error("削除対象拡張子を指定してください。");
                  setDeleteExec(null);
                  const result = await previewDelete({
                    inputPaths: deleteFiles,
                    includeSubfolders: deleteSubfolders,
                    extensions,
                    mode: deleteMode,
                    retreatDir: deleteMode === "retreat" ? deleteRetreatDir.trim() : null,
                    conflictPolicy: deleteConflictPolicy
                  });
                  if (result.total === 0) {
                    setDeletePreview(null);
                    addToast("info", "削除対象のファイルが見つかりませんでした。");
                  } else {
                    setDeletePreview(result);
                    addToast("success", `プレビュー完了: ${result.ready}/${result.total}件`);
                  }
                });
              }}
            >
              <div className="card">
                <h3 className="card-title">削除設定</h3>
                <div className="form-group">
                  <label className="form-label">削除パターン</label>
                  <div className="pattern-selector-row">
                    <select
                      value={isCreatingNewPattern ? "__new__" : patternSelected}
                      onChange={(event) => {
                        const val = event.target.value;
                        if (val === "__new__") {
                          setIsCreatingNewPattern(true);
                          setPatternSelected("");
                          setPatternOriginal(null);
                          setNewPatternName("");
                        } else {
                          setIsCreatingNewPattern(false);
                          setPatternSelected(val);
                          setNewPatternName("");
                          if (val) {
                            applyDeletePattern(val);
                          } else {
                            setPatternOriginal(null);
                          }
                        }
                      }}
                    >
                      <option value="">(選択しない)</option>
                      {patterns.map((pattern) => (
                        <option key={pattern.name} value={pattern.name}>
                          {pattern.name}
                        </option>
                      ))}
                      <option value="__new__">+ 新規パターン作成...</option>
                    </select>
                    {patternSelected && !isCreatingNewPattern ? (
                      <>
                        <button className="btn-icon" type="button" title="上書き保存" disabled={!patternModified} onClick={() => void overwritePattern()}>
                          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2z" /><polyline points="17 21 17 13 7 13 7 21" /><polyline points="7 3 7 8 15 8" /></svg>
                        </button>
                        <button className="btn-icon btn-icon-danger" type="button" title="パターン削除" onClick={() => void deletePatternByName()}>
                          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2" /></svg>
                        </button>
                      </>
                    ) : null}
                  </div>
                  {isCreatingNewPattern ? (
                    <div className="pattern-inline-create">
                      <input value={newPatternName} onChange={(event) => setNewPatternName(event.target.value)} placeholder="パターン名を入力" />
                      <button className="btn btn-sm btn-primary" type="button" onClick={() => void saveNewPattern()}>保存</button>
                      <button className="btn btn-sm" type="button" onClick={() => { setIsCreatingNewPattern(false); setNewPatternName(""); }}>キャンセル</button>
                    </div>
                  ) : null}
                  {patternModified && !isCreatingNewPattern ? (
                    <span className="form-hint" style={{ color: "var(--warning)" }}>未保存の変更があります</span>
                  ) : (
                    <span className="form-hint">選択すると、拡張子と削除方式がすぐ反映されます</span>
                  )}
                </div>
                <div className="form-group">
                  <label className="form-label">対象拡張子</label>
                  <textarea rows={2} value={deleteExtensions} onChange={(event) => setDeleteExtensions(event.target.value)} placeholder="例: jpg, tmp, bak" />
                  <span className="form-hint">カンマ・スペース・改行区切り。先頭ドットの有無は問いません</span>
                </div>
                <div className="form-row">
                  <div className="form-group">
                    <label className="form-label">削除方式</label>
                    <select value={deleteMode} onChange={(event) => setDeleteMode(event.target.value as DeletePattern["mode"])}>
                      <option value="direct">直接削除</option>
                      <option value="trash">ゴミ箱へ移動</option>
                      <option value="retreat">指定フォルダへ退避</option>
                    </select>
                  </div>
                  <div className="form-group">
                    <label className="form-label">競合時の処理</label>
                    <select value={deleteConflictPolicy} disabled={deleteMode === "trash"} onChange={(event) => setDeleteConflictPolicy(event.target.value as "overwrite" | "sequence" | "skip")}>
                      <option value="overwrite">上書き</option>
                      <option value="sequence">連番付与</option>
                      <option value="skip">スキップ</option>
                    </select>
                  </div>
                </div>
                {deleteMode === "retreat" ? (
                  <div className="form-group">
                    <label className="form-label">退避先フォルダ</label>
                    <input value={deleteRetreatDir} onChange={(event) => setDeleteRetreatDir(event.target.value)} placeholder="C:\retreat" />
                  </div>
                ) : null}
                <div className="form-group">
                  <label className="checkbox-row">
                    <input type="checkbox" checked={deleteSubfolders} onChange={(event) => setDeleteSubfolders(event.target.checked)} />
                    サブフォルダを含める
                  </label>
                </div>
              </div>

              {/* Actions */}
              <div className="page-actions">
                <button className="btn" disabled={isBusy}>プレビュー</button>
                <button
                  className="btn btn-primary"
                  type="button"
                  disabled={isBusy || !deleteFiles.length || !parseExts(deleteExtensions).length}
                  onClick={() =>
                    void run(async () => {
                      if (!deleteFiles.length) throw new Error("入力パスを指定してください。");
                      const extensions = parseExts(deleteExtensions);
                      if (!extensions.length) throw new Error("削除対象拡張子を指定してください。");
                      setProgressMap((prev) => { const next = { ...prev }; delete next.delete; return next; });
                      const result = await executeDelete({
                        inputPaths: deleteFiles,
                        includeSubfolders: deleteSubfolders,
                        extensions,
                        mode: deleteMode,
                        retreatDir: deleteMode === "retreat" ? deleteRetreatDir.trim() : null,
                        conflictPolicy: deleteConflictPolicy
                      });
                      setDeleteExec(result);
                      addToast("success", `削除完了: 成功${result.succeeded}件${result.failed > 0 ? ` / 失敗${result.failed}件` : ""}`);
                    })
                  }
                >
                  実行
                </button>
                {(deletePreview || deleteExec) ? (
                  <button className="btn" type="button" disabled={isBusy} onClick={() => { setDeletePaths(""); setDeletePreview(null); setDeleteExec(null); setProgressMap((prev) => { const next = { ...prev }; delete next.delete; return next; }); }}>クリア</button>
                ) : null}
              </div>
            </form>

            {/* Preview Results */}
            {deletePreview ? (
              <div className="result-section">
                {countConflictWarnings(deletePreview.items.map((item) => item.reason)) > 0 ? (
                  <div className="conflict-warning">
                    退避先で同名競合が検出されました。競合時の処理を確認して実行してください。
                  </div>
                ) : null}
                <div className="result-summary">
                  <span className="result-summary-label">プレビュー</span>
                  <span className="badge badge-info">{deletePreview.ready}/{deletePreview.total} 件</span>
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>対象</th>
                        <th style={{ width: 90 }}>処理</th>
                        <th>出力先</th>
                        <th style={{ width: 200 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {deletePreview.items.map((item) => (
                        <tr key={`${item.sourcePath}-${item.destinationPath ?? "none"}-${item.action}`}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td>{deleteActionLabel(item.action)}</td>
                          <td className="cell-path">{item.destinationPath ?? "-"}</td>
                          <td>{item.reason ?? "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}

            {/* Execute Results */}
            {deleteExec ? (
              <div className="result-section">
                <div className="result-summary">
                  <span className="result-summary-label">実行結果</span>
                  <span className="badge badge-success">成功 {deleteExec.succeeded}</span>
                  {deleteExec.failed > 0 ? <span className="badge badge-error">失敗 {deleteExec.failed}</span> : null}
                  {deleteExec.skipped > 0 ? <span className="badge badge-skip">スキップ {deleteExec.skipped}</span> : null}
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>対象</th>
                        <th style={{ width: 90 }}>処理</th>
                        <th>出力先</th>
                        <th style={{ width: 200 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {deleteExec.details.map((item) => (
                        <tr key={`${item.sourcePath}-${item.destinationPath ?? "none"}-${item.status}`}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td>{deleteActionLabel(item.action)}</td>
                          <td className="cell-path">{item.destinationPath ?? "-"}</td>
                          <td>{item.reason ?? "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}
          </>
        ) : null}

        {/* ===== Compress Tab ===== */}
        {tab === "compress" ? (
          <>
            <div className="input-row">
              {/* Drop Zone */}
              <div className={`drop-zone${isDragOver ? " drag-over" : ""}`}>
                <div className="drop-zone-icon"><DropIcon /></div>
                <div className="drop-zone-text">JPEGファイルまたはフォルダをここにドロップ</div>
                <div className="drop-zone-actions">
                  <button className="btn" type="button" onClick={() => void openFilesDialog()} disabled={isBusy}>ファイル選択</button>
                  <button className="btn" type="button" onClick={() => void openFolderDialog()} disabled={isBusy}>フォルダ選択</button>
                </div>
              </div>

              {/* Input Paths */}
              <div className="card">
                <div className="form-group">
                  <label className="form-label">入力パス {compressFiles.length > 0 ? <span className="text-muted">({compressFiles.length}件)</span> : null}</label>
                  <textarea className="paths-area" rows={3} value={compressPaths} onChange={(event) => setCompressPaths(event.target.value)} placeholder="パスを入力（改行またはカンマ区切り）" />
                </div>
              </div>
            </div>

            {/* Compress Settings */}
            <form
              onSubmit={(event: FormEvent) => {
                event.preventDefault();
                void run(async () => {
                  if (!compressFiles.length) throw new Error("入力パスを指定してください。");
                  setCompressExec(null);
                  const preview = await previewCompress({
                    inputPaths: compressFiles,
                    includeSubfolders: compressSubfolders,
                    resizePercent: compressResizePercent,
                    quality: compressQuality,
                    targetSizeKb: null,
                    tolerancePercent: compressTolerancePercent,
                    preserveExif: compressPreserveExif,
                    outputDir: compressOutputDir.trim() || null,
                    conflictPolicy: compressConflictPolicy
                  });
                  setCompressPreview(preview);
                  addToast("success", `プレビュー完了: ${preview.ready}/${preview.total}件`);
                }, "プレビュー中...");
              }}
            >
              <div className="card">
                <h3 className="card-title">圧縮設定</h3>
                <div className="form-row">
                  <div className="form-group">
                    <label className="form-label">リサイズ比率 (%)</label>
                    <div className="range-control">
                      <input
                        className="range-slider"
                        type="range"
                        min={1}
                        max={100}
                        value={compressResizePercent}
                        onChange={(event) => setCompressResizePercent(Math.max(1, Math.min(100, Number.parseInt(event.target.value, 10) || 1)))}
                      />
                      <span className="range-value">{compressResizePercent}%</span>
                    </div>
                  </div>
                  <div className="form-group">
                    <label className="form-label">圧縮品質 (1-100)</label>
                    <div className="range-control">
                      <input
                        className="range-slider"
                        type="range"
                        min={1}
                        max={100}
                        value={compressQuality}
                        onChange={(event) => setCompressQuality(Math.max(1, Math.min(100, Number.parseInt(event.target.value, 10) || 1)))}
                      />
                      <span className="range-value">{compressQuality}</span>
                    </div>
                  </div>
                </div>
                <div className="form-row">
                  <div className="form-group">
                    <label className="form-label">全体の目標サイズ</label>
                    <div className="target-size-row">
                      <input value={compressTargetSizeKb} onChange={(event) => setCompressTargetSizeKb(event.target.value)} placeholder="空欄で手動指定" />
                      <select className="target-size-unit" value={compressTargetSizeUnit} onChange={(event) => setCompressTargetSizeUnit(event.target.value as "GB" | "MB" | "KB")}>
                        <option value="GB">GB</option>
                        <option value="MB">MB</option>
                        <option value="KB">KB</option>
                      </select>
                      <button
                        className={`btn btn-sm${busyLabel === "計算中..." ? " btn-loading" : ""}`}
                        type="button"
                        disabled={isBusy || !compressFiles.length || !compressTargetSizeKb.trim()}
                        onClick={() =>
                          void run(async () => {
                            const parsed = parsePositiveInt(compressTargetSizeKb);
                            if (parsed === null) throw new Error("目標サイズは正の整数で指定してください。");
                            const targetSizeKb = parsed * UNIT_TO_KB[compressTargetSizeUnit];
                            setCompressExec(null);
                            const preview = await previewCompress({
                              inputPaths: compressFiles,
                              includeSubfolders: compressSubfolders,
                              resizePercent: compressResizePercent,
                              quality: compressQuality,
                              targetSizeKb,
                              tolerancePercent: compressTolerancePercent,
                              preserveExif: compressPreserveExif,
                              outputDir: compressOutputDir.trim() || null,
                              conflictPolicy: compressConflictPolicy
                            });
                            setCompressPreview(preview);
                            setCompressResizePercent(Math.round(preview.effectiveResizePercent));
                            setCompressQuality(preview.effectiveQuality);
                            addToast("success", `パラメータを自動計算しました: リサイズ ${preview.effectiveResizePercent.toFixed(1)}% / 品質 ${preview.effectiveQuality}`);
                          }, "計算中...")
                        }
                      >
                        {busyLabel === "計算中..." ? "計算中..." : "計算"}
                      </button>
                    </div>
                    <span className="form-hint">全ファイル合計の目標サイズ。「計算」でリサイズ・品質を自動調整</span>
                  </div>
                  <div className="form-group">
                    <label className="form-label">許容誤差 (%)</label>
                    <input type="number" min={0} value={compressTolerancePercent} onChange={(event) => setCompressTolerancePercent(Math.max(0, Number.parseFloat(event.target.value) || 0))} />
                  </div>
                </div>
                <div className="form-group">
                  <label className="form-label">出力先フォルダ</label>
                  <input value={compressOutputDir} onChange={(event) => setCompressOutputDir(event.target.value)} placeholder="空欄で自動作成（入力元と同階層）" />
                </div>
                <div className="form-group">
                  <label className="form-label">競合時の処理</label>
                  <select value={compressConflictPolicy} onChange={(event) => setCompressConflictPolicy(event.target.value as "overwrite" | "sequence" | "skip")}>
                    <option value="overwrite">上書き</option>
                    <option value="sequence">連番付与</option>
                    <option value="skip">スキップ</option>
                  </select>
                </div>
                <div className="form-group">
                  <label className="checkbox-row">
                    <input type="checkbox" checked={compressSubfolders} onChange={(event) => setCompressSubfolders(event.target.checked)} />
                    サブフォルダを含める
                  </label>
                  <label className="checkbox-row">
                    <input type="checkbox" checked={compressPreserveExif} onChange={(event) => setCompressPreserveExif(event.target.checked)} />
                    EXIFを保持
                  </label>
                </div>
              </div>

              {compressSizeSummary ? (
                <div className="size-summary">
                  <div className="size-summary-text">
                    元: {formatBytes(compressSizeSummary.totalSource)}
                    {compressSourceInfo ? ` (${compressSourceInfo.fileCount}件)` : ""}
                     → 推定: {formatBytes(compressSizeSummary.totalEstimated)}
                    {!compressSizeSummary.isSampled && estimateProgress ? (
                      <span className="estimate-sampling"> サンプリング中 ({estimateProgress.current}/{estimateProgress.total})</span>
                    ) : !compressSizeSummary.isSampled ? (
                      <span className="estimate-sampling"> (計算待ち)</span>
                    ) : null}
                    {compressSizeSummary.targetBytes !== null ? ` (目標: ${formatBytes(compressSizeSummary.targetBytes)})` : ""}
                    {compressSizeSummary.totalSource > 0 ? ` ${Math.round((compressSizeSummary.totalEstimated / compressSizeSummary.totalSource) * 100)}%` : ""}
                  </div>
                  {!compressSizeSummary.isSampled && estimateProgress ? (
                    <div className="size-summary-bar">
                      <div
                        className="size-summary-fill"
                        style={{ width: `${estimateProgress.total > 0 ? Math.round((estimateProgress.current / estimateProgress.total) * 100) : 0}%` }}
                      />
                    </div>
                  ) : (
                    <div className="size-summary-bar">
                      <div
                        className={`size-summary-fill${compressSizeSummary.targetBytes !== null && compressSizeSummary.totalEstimated > compressSizeSummary.targetBytes ? " size-summary-over" : ""}`}
                        style={{ width: `${compressSizeSummary.totalSource > 0 ? Math.min(100, Math.round((compressSizeSummary.totalEstimated / compressSizeSummary.totalSource) * 100)) : 0}%` }}
                      />
                      {compressSizeSummary.targetBytes !== null && compressSizeSummary.totalSource > 0 ? (
                        <div
                          className="size-summary-target"
                          style={{ left: `${Math.min(100, Math.round((compressSizeSummary.targetBytes / compressSizeSummary.totalSource) * 100))}%` }}
                        />
                      ) : null}
                    </div>
                  )}
                </div>
              ) : null}

              {/* Actions */}
              <div className="page-actions">
                <button className={`btn${busyLabel === "プレビュー中..." ? " btn-loading" : ""}`} disabled={isBusy}>
                  {busyLabel === "プレビュー中..." ? "プレビュー中..." : "プレビュー"}
                </button>
                <button
                  className={`btn btn-primary${busyLabel === "実行中..." ? " btn-loading" : ""}`}
                  type="button"
                  disabled={isBusy || !compressFiles.length}
                  onClick={() =>
                    void run(async () => {
                      if (!compressFiles.length) throw new Error("入力パスを指定してください。");
                      setProgressMap((prev) => { const next = { ...prev }; delete next.compress; return next; });
                      const result = await executeCompress({
                        inputPaths: compressFiles,
                        includeSubfolders: compressSubfolders,
                        resizePercent: compressResizePercent,
                        quality: compressQuality,
                        targetSizeKb: null,
                        tolerancePercent: compressTolerancePercent,
                        preserveExif: compressPreserveExif,
                        outputDir: compressOutputDir.trim() || null,
                        conflictPolicy: compressConflictPolicy
                      });
                      setCompressExec(result);
                      addToast("success", `圧縮完了: 成功${result.succeeded}件${result.failed > 0 ? ` / 失敗${result.failed}件` : ""}`);
                    }, "実行中...")
                  }
                >
                  {busyLabel === "実行中..." ? "実行中..." : "実行"}
                </button>
                {(compressPreview || compressExec) ? (
                  <button className="btn" type="button" disabled={isBusy} onClick={() => { setCompressPaths(""); setCompressSourceInfo(null); setCompressEstimateResult(null); setCompressPreview(null); setCompressExec(null); setProgressMap((prev) => { const next = { ...prev }; delete next.compress; return next; }); }}>クリア</button>
                ) : null}
              </div>
            </form>

            {/* Preview Results */}
            {compressPreview ? (
              <div className="result-section">
                {countConflictWarnings(compressPreview.items.map((item) => item.reason)) > 0 ? (
                  <div className="conflict-warning">
                    出力先で同名競合が検出されました。競合時の処理を確認して実行してください。
                  </div>
                ) : null}
                <div className="result-summary">
                  <span className="result-summary-label">プレビュー</span>
                  <span className="badge badge-info">{compressPreview.ready}/{compressPreview.total} 件</span>
                  {compressPreview.warnings > 0 ? <span className="badge badge-skip">警告 {compressPreview.warnings}</span> : null}
                  <span className="text-muted" style={{ fontSize: 12 }}>
                    リサイズ {compressPreview.effectiveResizePercent.toFixed(1)}% / 品質 {compressPreview.effectiveQuality}
                  </span>
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>入力</th>
                        <th>出力</th>
                        <th style={{ width: 90 }}>元サイズ</th>
                        <th style={{ width: 90 }}>推定サイズ</th>
                        <th style={{ width: 180 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {compressPreview.items.map((item) => (
                        <tr key={`${item.sourcePath}-${item.destinationPath}`}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td className="cell-path">{item.destinationPath}</td>
                          <td className="cell-size">{formatBytes(item.sourceSize)}</td>
                          <td className="cell-size">{formatBytes(item.estimatedSize)}</td>
                          <td>{item.reason ?? "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}

            {/* Execute Results */}
            {compressExec ? (
              <div className="result-section">
                <div className="result-summary">
                  <span className="result-summary-label">実行結果</span>
                  <span className="badge badge-success">成功 {compressExec.succeeded}</span>
                  {compressExec.failed > 0 ? <span className="badge badge-error">失敗 {compressExec.failed}</span> : null}
                  {compressExec.skipped > 0 ? <span className="badge badge-skip">スキップ {compressExec.skipped}</span> : null}
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>入力</th>
                        <th>出力</th>
                        <th style={{ width: 90 }}>出力サイズ</th>
                        <th style={{ width: 180 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {compressExec.details.map((item) => (
                        <tr key={`${item.sourcePath}->${item.destinationPath}-${item.status}`}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td className="cell-path">{item.destinationPath}</td>
                          <td className="cell-size">{item.outputSize ? formatBytes(item.outputSize) : "-"}</td>
                          <td>{item.reason ?? "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}
          </>
        ) : null}

        {/* ===== Flatten Tab ===== */}
        {tab === "flatten" ? (
          <>
            {/* Settings */}
            <form
              onSubmit={(event: FormEvent) => {
                event.preventDefault();
                void run(async () => {
                  if (!flattenInputDir.trim()) throw new Error("入力フォルダを指定してください。");
                  setFlattenExec(null);
                  const result = await previewFlatten({
                    inputDir: flattenInputDir.trim(),
                    outputDir: flattenOutputDir.trim() || null,
                    conflictPolicy: flattenConflictPolicy
                  });
                  setFlattenPreview(result);
                  addToast("success", `プレビュー完了: ${result.ready}/${result.total}件`);
                });
              }}
            >
              <div className="input-row">
                {/* Drop Zone */}
                <div className={`drop-zone${isDragOver ? " drag-over" : ""}`}>
                  <div className="drop-zone-icon"><DropIcon /></div>
                  <div className="drop-zone-text">フォルダをここにドロップ</div>
                  <div className="drop-zone-actions">
                    <button className="btn" type="button" onClick={() => void openFolderDialog()} disabled={isBusy}>フォルダ選択</button>
                  </div>
                </div>

                <div className="card">
                  <h3 className="card-title">展開設定</h3>
                <div className="form-group">
                  <label className="form-label">入力フォルダ</label>
                  <input value={flattenInputDir} onChange={(event) => setFlattenInputDir(event.target.value)} placeholder="展開するフォルダのパス" />
                </div>
                <div className="form-group">
                  <label className="form-label">出力先フォルダ</label>
                  <input value={flattenOutputDir} onChange={(event) => setFlattenOutputDir(event.target.value)} placeholder="空欄で自動作成（入力元と同階層）" />
                </div>
                <div className="form-group">
                  <label className="form-label">競合時の処理</label>
                  <select value={flattenConflictPolicy} onChange={(event) => setFlattenConflictPolicy(event.target.value as "overwrite" | "sequence" | "skip")}>
                    <option value="overwrite">上書き</option>
                    <option value="sequence">連番付与</option>
                    <option value="skip">スキップ</option>
                  </select>
                </div>
                </div>
              </div>

              {/* Actions */}
              <div className="page-actions">
                <button className="btn" disabled={isBusy}>プレビュー</button>
                <button
                  className="btn btn-primary"
                  type="button"
                  disabled={isBusy || !flattenInputDir.trim()}
                  onClick={() =>
                    void run(async () => {
                      if (!flattenInputDir.trim()) throw new Error("入力フォルダを指定してください。");
                      setProgressMap((prev) => { const next = { ...prev }; delete next.flatten; return next; });
                      const result = await executeFlatten({
                        inputDir: flattenInputDir.trim(),
                        outputDir: flattenOutputDir.trim() || null,
                        conflictPolicy: flattenConflictPolicy
                      });
                      setFlattenExec(result);
                      addToast("success", `展開完了: 成功${result.succeeded}件${result.failed > 0 ? ` / 失敗${result.failed}件` : ""}`);
                    })
                  }
                >
                  実行
                </button>
                {(flattenPreview || flattenExec) ? (
                  <button className="btn" type="button" disabled={isBusy} onClick={() => { setFlattenInputDir(""); setFlattenPreview(null); setFlattenExec(null); setProgressMap((prev) => { const next = { ...prev }; delete next.flatten; return next; }); }}>クリア</button>
                ) : null}
              </div>
            </form>

            {/* Preview Results */}
            {flattenPreview ? (
              <div className="result-section">
                {countConflictWarnings(flattenPreview.items.map((item) => item.reason)) > 0 ? (
                  <div className="conflict-warning">
                    出力先で同名競合が検出されました。競合時の処理を確認して実行してください。
                  </div>
                ) : null}
                <div className="result-summary">
                  <span className="result-summary-label">プレビュー</span>
                  <span className="badge badge-info">{flattenPreview.ready}/{flattenPreview.total} 件</span>
                  {flattenPreview.collisions > 0 ? <span className="badge badge-skip">競合 {flattenPreview.collisions}</span> : null}
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>入力</th>
                        <th>出力</th>
                        <th style={{ width: 200 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {flattenPreview.items.map((item) => (
                        <tr key={`${item.sourcePath}-${item.destinationPath}`}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td className="cell-path">{item.destinationPath}</td>
                          <td>{item.reason ?? "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}

            {/* Execute Results */}
            {flattenExec ? (
              <div className="result-section">
                <div className="result-summary">
                  <span className="result-summary-label">実行結果</span>
                  <span className="badge badge-success">成功 {flattenExec.succeeded}</span>
                  {flattenExec.failed > 0 ? <span className="badge badge-error">失敗 {flattenExec.failed}</span> : null}
                  {flattenExec.skipped > 0 ? <span className="badge badge-skip">スキップ {flattenExec.skipped}</span> : null}
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>入力</th>
                        <th>出力</th>
                        <th style={{ width: 200 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {flattenExec.details.map((item) => (
                        <tr key={`${item.sourcePath}-${item.destinationPath}-${item.status}`}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td className="cell-path">{item.destinationPath}</td>
                          <td>{item.reason ?? "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}
          </>
        ) : null}

        {/* ===== EXIF Offset Tab ===== */}
        {tab === "exif-offset" ? (
          <>
            <div className="input-row">
              {/* Drop Zone */}
              <div className={`drop-zone${isDragOver ? " drag-over" : ""}`}>
                <div className="drop-zone-icon"><DropIcon /></div>
                <div className="drop-zone-text">JPEGファイルまたはフォルダをここにドロップ</div>
                <div className="drop-zone-actions">
                  <button className="btn" type="button" onClick={() => void openFilesDialog()} disabled={isBusy}>ファイル選択</button>
                  <button className="btn" type="button" onClick={() => void openFolderDialog()} disabled={isBusy}>フォルダ選択</button>
                </div>
              </div>

              {/* Input Paths */}
              <div className="card">
                <div className="form-group">
                  <label className="form-label">入力パス {exifOffsetFiles.length > 0 ? <span className="text-muted">({exifOffsetFiles.length}件)</span> : null}</label>
                  <textarea className="paths-area" rows={3} value={exifOffsetPaths} onChange={(event) => setExifOffsetPaths(event.target.value)} placeholder="パスを入力（改行またはカンマ区切り）" />
                </div>
              </div>
            </div>

            {/* Settings */}
            <form
              onSubmit={(event: FormEvent) => {
                event.preventDefault();
                void run(async () => {
                  if (!exifOffsetFiles.length) throw new Error("入力パスを指定してください。");
                  if (totalOffsetSeconds === 0) throw new Error("オフセットを指定してください。");
                  setExifOffsetExec(null);
                  const result = await previewExifOffset({
                    inputPaths: exifOffsetFiles,
                    includeSubfolders: exifOffsetSubfolders,
                    offsetSeconds: totalOffsetSeconds
                  });
                  setExifOffsetPreview(result);
                  addToast("success", `プレビュー完了: ${result.ready}/${result.total}件`);
                });
              }}
            >
              <div className="card">
                <h3 className="card-title">オフセット設定</h3>
                <div className="form-group">
                  <label className="form-label">オフセット方向</label>
                  <select value={exifOffsetSign} onChange={(event) => setExifOffsetSign(event.target.value as "+" | "-")}>
                    <option value="+">+ 加算（日時を進める）</option>
                    <option value="-">- 減算（日時を戻す）</option>
                  </select>
                </div>
                <div className="form-group">
                  <label className="form-label">オフセット値</label>
                  <div className="form-row">
                    <div className="form-group">
                      <label className="form-label form-label-sm">日</label>
                      <input type="number" min={0} max={9999} value={exifOffsetDays} onChange={(event) => setExifOffsetDays(Math.max(0, Number.parseInt(event.target.value) || 0))} />
                    </div>
                    <div className="form-group">
                      <label className="form-label form-label-sm">時間</label>
                      <input type="number" min={0} max={23} value={exifOffsetHours} onChange={(event) => setExifOffsetHours(Math.max(0, Math.min(23, Number.parseInt(event.target.value) || 0)))} />
                    </div>
                    <div className="form-group">
                      <label className="form-label form-label-sm">分</label>
                      <input type="number" min={0} max={59} value={exifOffsetMinutes} onChange={(event) => setExifOffsetMinutes(Math.max(0, Math.min(59, Number.parseInt(event.target.value) || 0)))} />
                    </div>
                    <div className="form-group">
                      <label className="form-label form-label-sm">秒</label>
                      <input type="number" min={0} max={59} value={exifOffsetSeconds} onChange={(event) => setExifOffsetSeconds(Math.max(0, Math.min(59, Number.parseInt(event.target.value) || 0)))} />
                    </div>
                  </div>
                  <span className="form-hint">合計オフセット: {exifOffsetSign}{exifOffsetDays}日 {exifOffsetHours}時間 {exifOffsetMinutes}分 {exifOffsetSeconds}秒（{totalOffsetSeconds}秒）</span>
                </div>
                <div className="form-group">
                  <label className="form-check">
                    <input type="checkbox" checked={exifOffsetSubfolders} onChange={(event) => setExifOffsetSubfolders(event.target.checked)} />
                    サブフォルダを含める
                  </label>
                </div>
              </div>

              {/* Actions */}
              <div className="page-actions">
                <button className="btn" disabled={isBusy}>プレビュー</button>
                <button
                  className="btn btn-primary"
                  type="button"
                  disabled={isBusy || !exifOffsetFiles.length || totalOffsetSeconds === 0}
                  onClick={() =>
                    void run(async () => {
                      if (!exifOffsetFiles.length) throw new Error("入力パスを指定してください。");
                      if (totalOffsetSeconds === 0) throw new Error("オフセットを指定してください。");
                      setProgressMap((prev) => { const next = { ...prev }; delete next.exifOffset; return next; });
                      const result = await executeExifOffset({
                        inputPaths: exifOffsetFiles,
                        includeSubfolders: exifOffsetSubfolders,
                        offsetSeconds: totalOffsetSeconds
                      });
                      setExifOffsetExec(result);
                      addToast("success", `EXIF日時補正完了: 成功${result.succeeded}件${result.failed > 0 ? ` / 失敗${result.failed}件` : ""}`);
                    }, "実行中...")
                  }
                >
                  実行
                </button>
                {(exifOffsetPreview || exifOffsetExec) ? (
                  <button className="btn" type="button" disabled={isBusy} onClick={() => { setExifOffsetPaths(""); setExifOffsetPreview(null); setExifOffsetExec(null); setProgressMap((prev) => { const next = { ...prev }; delete next.exifOffset; return next; }); }}>クリア</button>
                ) : null}
              </div>
            </form>

            {/* Preview Results */}
            {exifOffsetPreview ? (
              <div className="result-section">
                <div className="result-summary">
                  <span className="result-summary-label">プレビュー</span>
                  <span className="badge badge-info">{exifOffsetPreview.ready}/{exifOffsetPreview.total} 件</span>
                  {exifOffsetPreview.skipped > 0 ? <span className="badge badge-skip">スキップ {exifOffsetPreview.skipped}</span> : null}
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>ファイル</th>
                        <th style={{ width: 180 }}>補正前日時</th>
                        <th style={{ width: 180 }}>補正後日時</th>
                        <th style={{ width: 200 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {exifOffsetPreview.items.map((item) => (
                        <tr key={item.sourcePath}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td>{item.originalDatetime ?? "-"}</td>
                          <td>{item.correctedDatetime ?? "-"}</td>
                          <td>{item.reason ?? "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}

            {/* Execute Results */}
            {exifOffsetExec ? (
              <div className="result-section">
                <div className="result-summary">
                  <span className="result-summary-label">実行結果</span>
                  <span className="badge badge-success">成功 {exifOffsetExec.succeeded}</span>
                  {exifOffsetExec.failed > 0 ? <span className="badge badge-error">失敗 {exifOffsetExec.failed}</span> : null}
                  {exifOffsetExec.skipped > 0 ? <span className="badge badge-skip">スキップ {exifOffsetExec.skipped}</span> : null}
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>ファイル</th>
                        <th style={{ width: 200 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {exifOffsetExec.details.map((item) => (
                        <tr key={`${item.sourcePath}-${item.status}`}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td>{item.reason ?? "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}
          </>
        ) : null}

        {/* ===== Metadata Strip Tab ===== */}
        {tab === "metadata-strip" ? (
          <>
            <div className="input-row">
              {/* Drop Zone */}
              <div className={`drop-zone${isDragOver ? " drag-over" : ""}`}>
                <div className="drop-zone-icon"><DropIcon /></div>
                <div className="drop-zone-text">JPEGファイルまたはフォルダをここにドロップ</div>
                <div className="drop-zone-actions">
                  <button className="btn" type="button" onClick={() => void openFilesDialog()} disabled={isBusy}>ファイル選択</button>
                  <button className="btn" type="button" onClick={() => void openFolderDialog()} disabled={isBusy}>フォルダ選択</button>
                </div>
              </div>

              {/* Input Paths */}
              <div className="card">
                <div className="form-group">
                  <label className="form-label">入力パス {metadataStripFiles.length > 0 ? <span className="text-muted">({metadataStripFiles.length}件)</span> : null}</label>
                  <textarea className="paths-area" rows={3} value={metadataStripPaths} onChange={(event) => setMetadataStripPaths(event.target.value)} placeholder="パスを入力（改行またはカンマ区切り）" />
                </div>
              </div>
            </div>

            {/* Settings */}
            <form
              onSubmit={(event: FormEvent) => {
                event.preventDefault();
                void run(async () => {
                  if (!metadataStripFiles.length) throw new Error("入力パスを指定してください。");
                  setMetadataStripExec(null);
                  const result = await previewMetadataStrip({
                    inputPaths: metadataStripFiles,
                    includeSubfolders: metadataStripSubfolders,
                    preset: metadataStripPreset,
                    categories: metadataStripCategories
                  });
                  setMetadataStripPreview(result);
                  addToast("success", `プレビュー完了: ${result.ready}/${result.total}件`);
                });
              }}
            >
              <div className="card">
                <h3 className="card-title">削除設定</h3>
                <div className="form-group">
                  <label className="form-label">プリセット</label>
                  <select
                    value={metadataStripPreset}
                    onChange={(event) => {
                      const preset = event.target.value as MetadataStripPreset;
                      setMetadataStripPreset(preset);
                      if (preset === "snsPublish") {
                        setMetadataStripCategories({ gps: true, cameraLens: true, software: false, authorCopyright: false, comments: true, thumbnail: true, iptc: false, xmp: false, shootingSettings: false, captureDateTime: false });
                      } else if (preset === "delivery") {
                        setMetadataStripCategories({ gps: false, cameraLens: true, software: true, authorCopyright: false, comments: true, thumbnail: false, iptc: false, xmp: false, shootingSettings: false, captureDateTime: false });
                      } else if (preset === "fullClean") {
                        setMetadataStripCategories({ gps: true, cameraLens: true, software: true, authorCopyright: true, comments: true, thumbnail: true, iptc: true, xmp: true, shootingSettings: true, captureDateTime: true });
                      }
                    }}
                  >
                    <option value="snsPublish">SNS公開用</option>
                    <option value="delivery">納品用</option>
                    <option value="fullClean">完全クリーン</option>
                    <option value="custom">カスタム</option>
                  </select>
                </div>
                <div className="form-group">
                  <label className="form-label">削除対象カテゴリ</label>
                  <div className="metadata-strip-categories">
                    {(
                      [
                        ["gps", "GPS/位置情報"],
                        ["cameraLens", "カメラ/レンズ情報"],
                        ["software", "作成ソフト/編集履歴"],
                        ["authorCopyright", "作者/著作権"],
                        ["comments", "コメント/説明"],
                        ["thumbnail", "サムネイル(IFD1)"],
                        ["iptc", "IPTC(APP13)"],
                        ["xmp", "XMP(APP1)"],
                        ["shootingSettings", "撮影時設定"],
                        ["captureDateTime", "撮影日時"]
                      ] as [keyof MetadataStripCategories, string][]
                    ).map(([key, label]) => (
                      <label key={key} className="form-check">
                        <input
                          type="checkbox"
                          checked={metadataStripCategories[key]}
                          onChange={(event) => {
                            setMetadataStripCategories((prev) => ({ ...prev, [key]: event.target.checked }));
                            setMetadataStripPreset("custom");
                          }}
                        />
                        {label}
                      </label>
                    ))}
                  </div>
                </div>
                <div className="form-group">
                  <label className="form-check">
                    <input type="checkbox" checked={metadataStripSubfolders} onChange={(event) => setMetadataStripSubfolders(event.target.checked)} />
                    サブフォルダを含める
                  </label>
                </div>
              </div>

              {/* Actions */}
              <div className="page-actions">
                <button className="btn" disabled={isBusy}>プレビュー</button>
                <button
                  className="btn btn-primary"
                  type="button"
                  disabled={isBusy || !metadataStripFiles.length}
                  onClick={() =>
                    void run(async () => {
                      if (!metadataStripFiles.length) throw new Error("入力パスを指定してください。");
                      setProgressMap((prev) => { const next = { ...prev }; delete next.metadataStrip; return next; });
                      const result = await executeMetadataStrip({
                        inputPaths: metadataStripFiles,
                        includeSubfolders: metadataStripSubfolders,
                        preset: metadataStripPreset,
                        categories: metadataStripCategories
                      });
                      setMetadataStripExec(result);
                      addToast("success", `個人情報削除完了: 成功${result.succeeded}件${result.failed > 0 ? ` / 失敗${result.failed}件` : ""}`);
                    }, "実行中...")
                  }
                >
                  実行
                </button>
                {(metadataStripPreview || metadataStripExec) ? (
                  <button className="btn" type="button" disabled={isBusy} onClick={() => { setMetadataStripPaths(""); setMetadataStripPreview(null); setMetadataStripExec(null); setProgressMap((prev) => { const next = { ...prev }; delete next.metadataStrip; return next; }); }}>クリア</button>
                ) : null}
              </div>
            </form>

            {/* Preview Results */}
            {metadataStripPreview ? (
              <div className="result-section">
                <div className="result-summary">
                  <span className="result-summary-label">プレビュー</span>
                  <span className="badge badge-info">{metadataStripPreview.ready}/{metadataStripPreview.total} 件</span>
                  {metadataStripPreview.skipped > 0 ? <span className="badge badge-skip">スキップ {metadataStripPreview.skipped}</span> : null}
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>ファイル</th>
                        <th>検出カテゴリ</th>
                        <th style={{ width: 72 }}>タグ数</th>
                        <th style={{ width: 56 }}>IPTC</th>
                        <th style={{ width: 56 }}>XMP</th>
                        <th style={{ width: 160 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {metadataStripPreview.items.map((item) => (
                        <tr key={item.sourcePath}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td>{item.foundCategories.join(", ") || "-"}</td>
                          <td style={{ textAlign: "center" }}>{item.tagsToStrip > 0 ? item.tagsToStrip : "-"}</td>
                          <td style={{ textAlign: "center" }}>{item.hasIptc ? "あり" : "-"}</td>
                          <td style={{ textAlign: "center" }}>{item.hasXmp ? "あり" : "-"}</td>
                          <td>{item.reason ?? "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}

            {/* Execute Results */}
            {metadataStripExec ? (
              <div className="result-section">
                <div className="result-summary">
                  <span className="result-summary-label">実行結果</span>
                  <span className="badge badge-success">成功 {metadataStripExec.succeeded}</span>
                  {metadataStripExec.failed > 0 ? <span className="badge badge-error">失敗 {metadataStripExec.failed}</span> : null}
                  {metadataStripExec.skipped > 0 ? <span className="badge badge-skip">スキップ {metadataStripExec.skipped}</span> : null}
                </div>
                <div className="table-container">
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 72 }}>状態</th>
                        <th>ファイル</th>
                        <th style={{ width: 72 }}>削除タグ数</th>
                        <th style={{ width: 160 }}>備考</th>
                      </tr>
                    </thead>
                    <tbody>
                      {metadataStripExec.details.map((item) => (
                        <tr key={`${item.sourcePath}-${item.status}`}>
                          <td className="cell-status"><StatusBadge status={item.status} /></td>
                          <td className="cell-path">{item.sourcePath}</td>
                          <td style={{ textAlign: "center" }}>{item.strippedTags > 0 ? item.strippedTags : "-"}</td>
                          <td>{item.reason ?? (item.strippedIptc || item.strippedXmp ? `${item.strippedIptc ? "IPTC " : ""}${item.strippedXmp ? "XMP " : ""}削除済み` : "-")}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}
          </>
        ) : null}

        {/* ===== Settings Tab ===== */}
        {tab === "settings" ? (
          <>
            {/* Appearance */}
            <div className="card">
              <h3 className="card-title">外観</h3>
              <div className="form-group">
                <label className="form-label">テーマ</label>
                <select value={settings.theme} onChange={(event) => setSettings((current) => ({ ...current, theme: event.target.value as AppSettings["theme"] }))}>
                  <option value="system">システム設定に従う</option>
                  <option value="light">ライト</option>
                  <option value="dark">ダーク</option>
                </select>
              </div>
              <div style={{ display: "flex", gap: 8, marginTop: 12 }}>
                <button className="btn btn-primary" type="button" onClick={() => void run(async () => { const next = withOutputDirectories(settings); applySettingsToInputs(next); await saveSettings(next); setSettingsStatus("設定を保存しました。"); })}>設定保存</button>
                <button className="btn" type="button" onClick={() => void run(async () => { await openSettingsFolder(); setSettingsStatus("設定フォルダを開きました。"); })}>設定フォルダを開く</button>
              </div>
            </div>

            {/* Export */}
            <div className="card">
              <h3 className="card-title">エクスポート</h3>
              <div className="form-group">
                <label className="form-label">エクスポート先パス</label>
                <input value={exportPath} onChange={(event) => setExportPath(event.target.value)} placeholder="C:\backup\settings.json" />
              </div>
              <div style={{ marginTop: 8 }}>
                <button className="btn" type="button" onClick={() => void run(async () => { if (!exportPath.trim()) throw new Error("エクスポート先パスを指定してください。"); await exportSettings(exportPath.trim()); setSettingsStatus("設定をエクスポートしました。"); })}>JSONエクスポート</button>
              </div>
            </div>

            {/* Import */}
            <div className="card">
              <h3 className="card-title">インポート</h3>
              <div className="form-group">
                <label className="form-label">インポート元パス</label>
                <input value={importPath} onChange={(event) => { setImportPath(event.target.value); setImportConflictPreview(null); }} placeholder="C:\backup\settings.json" />
              </div>
              <div className="form-row">
                <div className="form-group">
                  <label className="form-label">インポート方式</label>
                  <select value={importMode} onChange={(event) => { setImportMode(event.target.value as "overwrite" | "merge"); setImportConflictPreview(null); }}>
                    <option value="overwrite">上書き</option>
                    <option value="merge">マージ</option>
                  </select>
                </div>
                {importMode === "merge" ? (
                  <div className="form-group">
                    <label className="form-label">衝突時の方針</label>
                    <select value={importConflictPolicy} onChange={(event) => setImportConflictPolicy(event.target.value as "existing" | "import" | "cancel")}>
                      <option value="existing">既存設定を優先</option>
                      <option value="import">インポート設定を優先</option>
                      <option value="cancel">キャンセル</option>
                    </select>
                  </div>
                ) : <div />}
              </div>
              {importMode === "merge" ? (
                <div style={{ marginTop: 8 }}>
                  <button className="btn btn-sm" type="button" onClick={() => void run(async () => { await refreshImportConflicts(); })}>衝突確認</button>
                  {importConflictPreview && hasImportConflicts(importConflictPreview) ? (
                    <div className="import-conflicts">
                      <p><strong>衝突が見つかりました。方針を選んで実行してください。</strong></p>
                      <p>削除パターン: {importConflictPreview.deletePatternNames.join(", ") || "なし"}</p>
                      <p>リネームテンプレート: {importConflictPreview.renameTemplateNames.join(", ") || "なし"}</p>
                      <p>出力先キー: {importConflictPreview.outputDirectoryKeys.join(", ") || "なし"}</p>
                      <p>テーマ: {importConflictPreview.themeConflict ? "衝突あり" : "衝突なし"}</p>
                    </div>
                  ) : null}
                </div>
              ) : null}
              <div style={{ marginTop: 12 }}>
                <button className="btn btn-primary" type="button" onClick={() => void run(async () => { if (!importPath.trim()) throw new Error("インポート元パスを指定してください。"); if (importMode === "merge" && !importConflictPreview) { const checked = await refreshImportConflicts(); if (hasImportConflicts(checked)) { throw new Error("衝突が見つかりました。方針を確認して再実行してください。"); } } const next = await importSettings(importPath.trim(), importMode, importConflictPolicy); applySettingsToInputs(next); setSettingsStatus("設定をインポートしました。"); })}>JSONインポート</button>
              </div>
            </div>

            {/* Settings Info */}
            <div className="settings-info">
              設定ファイル: {settingsPath || "(読み込み中)"}
            </div>
            {settingsStatus ? <div className="settings-status">{settingsStatus}</div> : null}
          </>
        ) : null}

        {/* ===== About Tab ===== */}
        {tab === "about" ? (
          <>
            <div className="card">
              <h3 className="card-title">Creators File Manager</h3>
              <p>バージョン: 0.1.0</p>
              <p>映像・写真クリエイター向けのファイル操作ユーティリティです。</p>
            </div>

            <div className="card">
              <h3 className="card-title">機能一覧</h3>
              <ul style={{ paddingLeft: 20 }}>
                <li><strong>一括リネーム</strong> — 撮影日時・更新日時・テンプレートでファイル名を一括変更</li>
                <li><strong>拡張子一括削除</strong> — 指定拡張子のファイルを直接削除・ゴミ箱移動・退避フォルダ移動</li>
                <li><strong>JPEG一括圧縮</strong> — リサイズ比率・品質指定で一括圧縮、目標サイズ自動計算</li>
                <li><strong>EXIF日時補正</strong> — JPEGのEXIF撮影日時をオフセット補正</li>
                <li><strong>メタデータ削除</strong> — JPEGのEXIFからGPS・カメラ情報などを一括削除</li>
                <li><strong>フォルダ展開</strong> — フォルダ構造を展開し、すべてのファイルをフラットにコピー</li>
              </ul>
            </div>

          </>
        ) : null}
      </main>

      {/* Toast Notifications */}
      {toasts.length > 0 ? (
        <div className="toast-container">
          {toasts.map((toast) => (
            <ToastItem key={toast.id} toast={toast} onDismiss={removeToast} />
          ))}
        </div>
      ) : null}
    </div>
  );
}
