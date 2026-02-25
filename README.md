<p align="center">
  <img src="public/CF.png" width="96" alt="Creator's File Manager icon">
</p>

<h1 align="center">Creator's File Manager</h1>

<p align="center">
  映像・写真クリエイター向けの、ファイル一括処理デスクトップアプリ
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.1.0-blue" alt="version">
  <img src="https://img.shields.io/badge/platform-Windows-0078d4" alt="platform">
  <img src="https://img.shields.io/badge/Tauri-v2-FFC131" alt="tauri">
  <img src="https://img.shields.io/badge/Rust-backend-CE422B" alt="rust">
</p>

---

## 概要

**Creator's File Manager** は、映像・写真制作者が日常的に行うファイル操作を高速・一括処理するための Windows デスクトップアプリです。

フォルダやファイルをドラッグ＆ドロップするだけで、面倒な大量ファイルの処理を一瞬でこなせます。

---

## 機能

### 🏷️ 一括リネーム

動画・画像ファイルの名前を、撮影日時・更新日時・任意テンプレートで一括変更します。

- **撮影日時** を EXIF / 動画メタデータから自動取得（取得不可時はファイル更新日時にフォールバック）
- **テンプレート構文** で柔軟にカスタマイズ：`{date:YYYYMMDD}_{time:HHmmss}_{seq:3}` など
- 実行前に「変更前 → 変更後」を**プレビュー**確認
- サブフォルダ再帰対応・競合時は連番サフィックスを自動付与

**対応形式：**
画像 `.jpg .jpeg .png .webp .gif .tif .tiff .bmp .heic .heif .dng .cr2 .cr3 .nef .arw .raf`
動画 `.mp4 .mov .m4v .avi .mkv .wmv .mts .m2ts .mpg .mpeg .webm`

---

### 🗑️ 拡張子一括削除

指定した拡張子に一致するファイルを、名前付き**削除パターン**で一括処理します。

- **削除方式を選択**：直接削除 / ゴミ箱へ移動 / 指定フォルダへ退避
- よく使うパターンを登録・保存して呼び出し可能
- 実行前に対象ファイル一覧と件数を確認表示
- サブフォルダ再帰対応

---

### 🖼️ JPEG 一括圧縮

JPEG ファイルをリサイズ比率・品質指定で一括圧縮します。

- 処理前に**推定ファイルサイズを表示**
- **目標サイズ指定**でリサイズ比率・品質を自動計算
- EXIF 情報の保持 / 除去を選択可能
- 出力先フォルダを自動生成（`{dirname}_compressed_{日時}`）
- Rust ネイティブ処理による高速並列圧縮

---

### 📂 フォルダ展開（フラット化）

フォルダ配下の全ファイルを、階層なしで出力フォルダへコピーします。

```
project/
  day1/
    video.mp4
  day2/
    clip.mp4
    photo.jpg
↓
output/
  video.mp4
  clip.mp4
  photo.jpg
```

- 競合ファイルの処理方式を事前に選択：**上書き / 連番付与 / スキップ**
- 実行前に競合ファイルをプレビュー確認
- 出力先フォルダを自動生成（`{dirname}_flattened_{日時}`）

---

### 🕐 EXIF 日時オフセット

JPEG ファイルの撮影日時を、指定した時間分だけ一括でずらします。

- 時差補正や誤った時刻設定の修正に活用
- 実行前にプレビューで変更後の日時を確認
- サブフォルダ再帰対応

---

### 🧹 メタデータ削除

JPEG ファイルから EXIF 等のメタデータを一括で除去します。

- SNS 投稿前の位置情報・個人情報の除去に
- 削除するメタデータのカテゴリを選択可能
- サブフォルダ再帰対応

---

## スクリーンショット

<!-- TODO: スクリーンショットを追加してください -->
> スクリーンショットは近日公開予定です。

---

## ダウンロード・インストール

[Releases](../../releases) ページから最新版のインストーラーをダウンロードしてください。

| ファイル | 説明 |
|---|---|
| `Creator's File Manager_*-setup.exe` | NSIS インストーラー（推奨） |
| `Creator's File Manager_*.msi` | MSI インストーラー |

インストーラーを実行してウィザードに従うだけで使用できます。

> **動作環境：** Windows 10 / 11（64bit）

---

## 使い方

1. アプリを起動し、左サイドバーから処理したい機能を選択
2. ファイルやフォルダをドロップゾーンへドラッグ＆ドロップ
3. パラメータを設定して「プレビュー」で確認
4. 内容を確認したら「実行」で処理開始

> 大量ファイルの処理中は進捗バーでリアルタイムに状況を確認できます。キャンセルも可能です。

---

## 設定・データ保存先

アプリの設定（削除パターン・テンプレート等）は以下に保存されます：

```
%APPDATA%\com.local.creator-file-manager\
```

設定のインポート / エクスポートは「設定」ページから行えます。

---

## ライセンス

[MIT License](LICENSE)
