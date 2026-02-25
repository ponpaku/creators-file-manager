export type RenameSource = "captureThenModified" | "modifiedOnly" | "currentTime";

export interface RenamePreviewRequest {
  inputPaths: string[];
  includeSubfolders: boolean;
  template: string;
  source: RenameSource;
  outputDir?: string | null;
  conflictPolicy?: "overwrite" | "sequence" | "skip" | null;
  useFfprobe?: boolean | null;
}

export interface RenamePreviewItem {
  sourcePath: string;
  destinationPath?: string | null;
  status: "ready" | "skipped";
  reason?: string | null;
}

export interface RenamePreviewResponse {
  items: RenamePreviewItem[];
  total: number;
  ready: number;
  skipped: number;
}

export interface RenameExecuteDetail {
  sourcePath: string;
  destinationPath?: string | null;
  status: "succeeded" | "failed" | "skipped";
  reason?: string | null;
}

export interface RenameExecuteResponse {
  processed: number;
  succeeded: number;
  failed: number;
  skipped: number;
  details: RenameExecuteDetail[];
}

export interface RenameTemplateTag {
  token: string;
  label: string;
  description: string;
}

export interface RenameTemplate {
  name: string;
  template: string;
}

export interface DeletePattern {
  name: string;
  extensions: string[];
  mode: "direct" | "trash" | "retreat";
  retreatDir?: string | null;
}

export interface DeletePreviewRequest {
  inputPaths: string[];
  includeSubfolders: boolean;
  extensions: string[];
  mode: "direct" | "trash" | "retreat";
  retreatDir?: string | null;
  conflictPolicy?: "overwrite" | "sequence" | "skip" | null;
}

export interface DeletePreviewItem {
  sourcePath: string;
  action: string;
  destinationPath?: string | null;
  status: "ready" | "skipped";
  reason?: string | null;
}

export interface DeletePreviewResponse {
  items: DeletePreviewItem[];
  total: number;
  ready: number;
  skipped: number;
}

export interface DeleteExecuteDetail {
  sourcePath: string;
  action: string;
  destinationPath?: string | null;
  status: "succeeded" | "failed" | "skipped";
  reason?: string | null;
}

export interface DeleteExecuteResponse {
  processed: number;
  succeeded: number;
  failed: number;
  skipped: number;
  details: DeleteExecuteDetail[];
}

export interface FlattenPreviewRequest {
  inputDir: string;
  outputDir?: string | null;
  conflictPolicy: "overwrite" | "sequence" | "skip";
}

export interface FlattenPreviewItem {
  sourcePath: string;
  destinationPath: string;
  status: "ready" | "skipped";
  reason?: string | null;
}

export interface FlattenPreviewResponse {
  outputDir: string;
  items: FlattenPreviewItem[];
  total: number;
  ready: number;
  skipped: number;
  collisions: number;
}

export interface FlattenExecuteDetail {
  sourcePath: string;
  destinationPath: string;
  status: "succeeded" | "failed" | "skipped";
  reason?: string | null;
}

export interface FlattenExecuteResponse {
  outputDir: string;
  processed: number;
  succeeded: number;
  failed: number;
  skipped: number;
  details: FlattenExecuteDetail[];
}

export interface CompressCollectInfoResponse {
  fileCount: number;
  totalSize: number;
}

export interface CompressEstimateResponse {
  fileCount: number;
  totalSourceSize: number;
  estimatedTotalSize: number;
}

export interface EstimateProgressEvent {
  current: number;
  total: number;
}

export interface CompressPreviewRequest {
  inputPaths: string[];
  includeSubfolders: boolean;
  resizePercent: number;
  quality: number;
  targetSizeKb?: number | null;
  tolerancePercent?: number | null;
  preserveExif: boolean;
  outputDir?: string | null;
  conflictPolicy: "overwrite" | "sequence" | "skip";
}

export interface CompressPreviewItem {
  sourcePath: string;
  destinationPath: string;
  sourceSize: number;
  estimatedSize: number;
  status: "ready" | "skipped";
  reason?: string | null;
}

export interface CompressPreviewResponse {
  outputDir: string;
  effectiveResizePercent: number;
  effectiveQuality: number;
  targetSizeKb?: number | null;
  tolerancePercent: number;
  items: CompressPreviewItem[];
  total: number;
  ready: number;
  skipped: number;
  warnings: number;
}

export interface CompressExecuteDetail {
  sourcePath: string;
  destinationPath: string;
  status: "succeeded" | "failed" | "skipped";
  outputSize?: number | null;
  reason?: string | null;
}

export interface CompressExecuteResponse {
  outputDir: string;
  effectiveResizePercent: number;
  effectiveQuality: number;
  processed: number;
  succeeded: number;
  failed: number;
  skipped: number;
  details: CompressExecuteDetail[];
}

export interface ExifOffsetPreviewRequest {
  inputPaths: string[];
  includeSubfolders: boolean;
  offsetSeconds: number;
}

export interface ExifOffsetPreviewItem {
  sourcePath: string;
  originalDatetime?: string | null;
  correctedDatetime?: string | null;
  status: "ready" | "skipped";
  reason?: string | null;
}

export interface ExifOffsetPreviewResponse {
  items: ExifOffsetPreviewItem[];
  total: number;
  ready: number;
  skipped: number;
}

export interface ExifOffsetExecuteDetail {
  sourcePath: string;
  status: "succeeded" | "failed" | "skipped";
  reason?: string | null;
}

export interface ExifOffsetExecuteResponse {
  processed: number;
  succeeded: number;
  failed: number;
  skipped: number;
  details: ExifOffsetExecuteDetail[];
}

export interface MetadataStripCategories {
  gps: boolean;
  cameraLens: boolean;
  software: boolean;
  authorCopyright: boolean;
  comments: boolean;
  thumbnail: boolean;
  iptc: boolean;
  xmp: boolean;
  shootingSettings: boolean;
  captureDateTime: boolean;
}

export type MetadataStripPreset = "snsPublish" | "delivery" | "fullClean" | "custom";

export interface MetadataStripPreviewRequest {
  inputPaths: string[];
  includeSubfolders: boolean;
  preset: MetadataStripPreset;
  categories: MetadataStripCategories;
}

export interface MetadataStripPreviewItem {
  sourcePath: string;
  foundCategories: string[];
  tagsToStrip: number;
  hasIptc: boolean;
  hasXmp: boolean;
  status: "ready" | "skipped";
  reason?: string | null;
}

export interface MetadataStripPreviewResponse {
  items: MetadataStripPreviewItem[];
  total: number;
  ready: number;
  skipped: number;
}

export interface MetadataStripExecuteDetail {
  sourcePath: string;
  strippedTags: number;
  strippedIptc: boolean;
  strippedXmp: boolean;
  status: "succeeded" | "failed" | "skipped";
  reason?: string | null;
}

export interface MetadataStripExecuteResponse {
  processed: number;
  succeeded: number;
  failed: number;
  skipped: number;
  details: MetadataStripExecuteDetail[];
}

export interface OperationProgressEvent {
  operation: "rename" | "delete" | "flatten" | "compress" | "exifOffset" | "metadataStrip";
  processed: number;
  total: number;
  succeeded: number;
  failed: number;
  skipped: number;
  currentPath?: string | null;
  done: boolean;
  canceled: boolean;
}

export interface AppSettings {
  deletePatterns: DeletePattern[];
  renameTemplates: RenameTemplate[];
  outputDirectories: Record<string, string>;
  theme: "system" | "light" | "dark";
}

export interface ImportConflictPreview {
  deletePatternNames: string[];
  renameTemplateNames: string[];
  outputDirectoryKeys: string[];
  themeConflict: boolean;
}
