# Creator's File Manager — エージェント向けプロジェクトガイド

## 1. プロジェクト概要

クリエイター（映像・写真制作者）向けのデスクトップファイル操作ユーティリティ。
6 つの一括処理機能を提供する Windows ネイティブアプリケーション。

| 機能 | 概要 |
|---|---|
| 一括リネーム | 撮影日時 / 更新日時 / テンプレートでファイル名を一括変更 |
| 拡張子一括削除 | 指定拡張子のファイルを直接削除 / ゴミ箱移動 / 退避フォルダ移動 |
| JPEG 一括圧縮 | リサイズ比率・品質指定、目標サイズ自動計算 |
| フォルダ展開 | 再帰的にファイルをフラットにコピー |
| EXIF 日時オフセット | JPEG の撮影日時を指定時間分だけ一括でずらす |
| メタデータ削除 | JPEG から EXIF 等のメタデータを一括除去 |

初期計画は `plan.md` を参照。

## 2. 技術スタック

| レイヤー | 技術 | バージョン |
|---|---|---|
| デスクトップフレームワーク | Tauri v2 | 2.0 |
| バックエンド | Rust (edition 2021) | — |
| フロントエンド | TypeScript + React | TS 5.6 / React 18.3 |
| バンドラー | Vite | 6.0 |
| 画像処理 | `image` クレート | 0.25 |
| EXIF | `kamadak-exif` | 0.6 |
| 並列処理 | `rayon` | 1 |
| ファイル削除 | `trash` | 5 |
| Windows API | `windows-sys` | 0.59 |

## 3. ディレクトリ構成

```
file-manager/
├── src/                          # フロントエンド (TypeScript/React)
│   ├── main.tsx                  # エントリポイント
│   ├── App.tsx                   # メインコンポーネント (全 UI)
│   ├── api.ts                    # Tauri invoke ラッパー
│   ├── types.ts                  # 共有型定義
│   └── styles.css                # 全スタイル (CSS 変数ベース)
├── src-tauri/                    # バックエンド (Rust)
│   ├── src/
│   │   ├── main.rs               # エントリ (console 非表示)
│   │   ├── lib.rs                # Tauri コマンドハンドラ + run()
│   │   ├── model.rs              # 全データモデル / Serde 定義
│   │   ├── error.rs              # AppError (thiserror)
│   │   ├── rename.rs             # リネームロジック
│   │   ├── delete.rs             # 削除ロジック
│   │   ├── compress.rs           # JPEG 圧縮ロジック
│   │   ├── flatten.rs            # フォルダ展開ロジック
│   │   ├── exif_offset.rs        # EXIF 日時オフセットロジック
│   │   ├── metadata_strip.rs     # メタデータ削除ロジック
│   │   ├── file_collect.rs       # ファイル収集・拡張子フィルタ
│   │   ├── fs_atomic.rs          # 原子的ファイル操作
│   │   ├── path_norm.rs          # パス正規化
│   │   └── settings.rs           # 設定 CRUD / インポート / エクスポート
│   ├── Cargo.toml
│   └── tauri.conf.json
├── dist/                         # Vite ビルド出力
├── index.html                    # HTML エントリ
├── package.json
├── tsconfig.json
├── vite.config.ts
├── requirements.md               # 要件定義書
└── plan.md                       # 開発計画
```

## 4. アーキテクチャ

### フロントエンド → バックエンド通信

Tauri IPC (`invoke`) を使用。`api.ts` が全コマンドをラップし、`types.ts` で型安全性を担保。

```
App.tsx → api.ts (invoke) → lib.rs (コマンドハンドラ) → 各モジュール
```

### バックエンドモジュール責務

| モジュール | 責務 |
|---|---|
| `lib.rs` | コマンド登録・ディスパッチ、キャンセルフラグ (`CANCEL_REQUESTED`) |
| `model.rs` | 全リクエスト / レスポンス型、設定モデル |
| `rename.rs` | EXIF / 動画メタデータ読取、テンプレート展開、リネーム実行 |
| `delete.rs` | 拡張子マッチ、3 モード削除 (direct / trash / retreat) |
| `compress.rs` | JPEG リサイズ・品質調整、目標サイズ逆算 |
| `flatten.rs` | 再帰走査→フラットコピー、衝突検出 |
| `exif_offset.rs` | EXIF 日時タグの読取・オフセット計算・書換 |
| `metadata_strip.rs` | JPEG バイト列からメタデータセグメントを除去 |
| `file_collect.rs` | walkdir ベースのファイル収集、拡張子フィルタ |
| `fs_atomic.rs` | 一時ファイル経由の原子的書換 (`ReplaceFileW` 優先) |
| `path_norm.rs` | ドライブ文字 / UNC 正規化、相対パス算出 |
| `settings.rs` | JSON 永続化、マージ / 衝突検出、フォルダオープン |

### フロントエンド構成

- **単一コンポーネント**: `App.tsx` に全 UI を集約
- **レイアウト**: サイドバー (`aside.sidebar`) + メインコンテンツ (`main.main-content`)
- **状態管理**: React hooks (`useState`, `useEffect`, `useMemo`, `useRef`)
- **スタイル**: CSS 変数ベース、ライト / ダーク / システムテーマ対応
- **ドラッグ & ドロップ**: Tauri `onDragDropEvent` で処理、`isDragOver` で視覚フィードバック

### UI デザイン方針

- Windows 11 Settings 風のサイドバーナビゲーション
- Fluent Design 風カラーパレット (アクセント `#0078d4`)
- カード (`.card`) でセクション分離
- フォームは必ずラベル (`.form-label`) + ヒント (`.form-hint`) 付き
- ボタン階層: プライマリ (`.btn-primary`, 青) / セカンダリ (`.btn`) / デンジャー (`.btn-danger`, 赤)
- テーブル: ゼブラストライプ、スティッキーヘッダー、ステータスバッジ
- ドロップゾーン: 破線枠、ドラッグオーバー時に色変化

## 5. ビルド・開発コマンド

```bash
# 開発サーバー起動 (フロントのみ、ポート 1420)
npm run dev

# Tauri 開発モード (フロント + バックエンド)
npm run tauri dev

# TypeScript 型チェック
npx tsc --noEmit

# フロントエンドビルド
npm run build

# リリースビルド (MSI + NSIS インストーラー生成)
npx tauri build
```

### ビルド出力

| 成果物 | パス |
|---|---|
| 実行ファイル | `src-tauri/target/release/file-manager.exe` |
| MSI | `src-tauri/target/release/bundle/msi/Creator's File Manager_*.msi` |
| NSIS | `src-tauri/target/release/bundle/nsis/Creator's File Manager_*-setup.exe` |

## 6. データフロー

### プレビュー → 実行フロー

各機能は「プレビュー → 確認 → 実行」の 2 ステップ:

1. ユーザーがパラメータを設定し「プレビュー」ボタンを押す
2. バックエンドが `preview_*` コマンドでドライランし、結果一覧を返す
3. ユーザーが結果を確認後「実行」ボタンを押す
4. バックエンドが `execute_*` コマンドで実処理
5. 進捗は `operation-progress` イベントでリアルタイム通知

### 設定の永続化

- 保存先: Tauri 標準アプリ設定ディレクトリ (`%APPDATA%/<app-name>/`)
- 形式: JSON
- 保存タイミング: 出力先変更は 400ms デバウンス後に自動保存
- ウィンドウ状態: `localStorage` に保存

## 7. コーディング規約

### Rust

- エラー型は `AppError` に統一、`thiserror` で定義
- ファイル操作は `fs_atomic.rs` の関数を使用（直接 `fs::rename` / `fs::copy` しない）
- 並列処理は `rayon` を使用
- 進捗コールバックは `impl Fn(usize)` で受け取る
- パス正規化は `path_norm.rs` を経由

### TypeScript / React

- 全コンポーネントは関数コンポーネント
- Tauri コマンド呼出しは必ず `api.ts` 経由（直接 `invoke` しない）
- 型は `types.ts` に集約（バックエンド `model.rs` と対応）
- CSS クラス名はケバブケース
- インラインスタイルは最小限（レイアウト微調整のみ許可）

### 命名規則

| 対象 | 規則 | 例 |
|---|---|---|
| Rust 関数 / 変数 | snake_case | `preview_rename` |
| Rust 型 | PascalCase | `RenamePreviewRequest` |
| TS 関数 / 変数 | camelCase | `previewRename` |
| TS 型 | PascalCase | `RenamePreviewResponse` |
| CSS クラス | kebab-case | `.drop-zone-icon` |
| Tauri コマンド | snake_case | `preview_rename` |

## 8. 新機能追加時のチェックリスト

1. `model.rs` にリクエスト / レスポンス型を追加
2. `types.ts` に対応する TypeScript 型を追加
3. バックエンドロジックを新規モジュールまたは既存モジュールに実装
4. `lib.rs` にコマンドハンドラを追加し `generate_handler!` に登録
5. `api.ts` に invoke ラッパー関数を追加
6. `App.tsx` に UI を追加（ドロップゾーン → 入力 → 設定カード → アクション → 結果テーブル）
7. `styles.css` に必要なスタイルを追加（CSS 変数を活用）
8. `npx tsc --noEmit` で型チェック通過を確認

## 9. 対応ファイル形式

### リネーム対象

画像: `.jpg .jpeg .png .webp .gif .tif .tiff .bmp .heic .heif .dng .cr2 .cr3 .nef .arw .raf`
動画: `.mp4 .mov .m4v .avi .mkv .wmv .mts .m2ts .mpg .mpeg .webm`

### 圧縮対象 / EXIF 日時オフセット対象 / メタデータ削除対象

`.jpg .jpeg` のみ

## 10. 注意事項

- UI 言語は日本語（将来 i18n 対応可能な構造）
- 最小ウィンドウサイズ: 1280x720
- キャンセル時はロールバックしない（処理済みファイルはそのまま保持）
- 自動作成フォルダ名の重複時は `_no1`, `_no2` を付与
- 上書き系処理は一時ファイル → 原子的置換（失敗時に元ファイルを保護）
