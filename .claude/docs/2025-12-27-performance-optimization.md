# Performance Optimization Opportunities

調査日: 2025-12-27

## 概要

svt の高速化余地を調査した結果をまとめる。

## 既に実装済みの最適化

| 項目 | 説明 | 場所 |
|------|------|------|
| スレッド分離 | Worker/Writer/Main の3スレッド構成 | 全体 |
| 最新リクエスト優先 | `drain_to_latest()` で古いリクエスト破棄 | worker.rs |
| ナビゲーション遅延 | `nav_latch` で高速ナビ時の処理スキップ | main.rs |
| エポック管理 | 古い非同期タスクの無視 | app.rs, sender.rs |
| SIMD Base64 | `base64_simd` クレート使用 | kgp.rs |
| 矩形差分計算 | dirty area 管理 | sender.rs |
| Zlib 圧縮 | KGP 転送データの圧縮 | kgp.rs |

---

## Tier 1: 高インパクト & 低〜中複雑度

### 1. DynamicImage の Arc 化

**現状の問題:**
```rust
// worker.rs:125,129,138
img.clone()  // DynamicImage 全体をメモリにフルコピー
```

**改善案:**
```rust
type ImageCache = Option<(PathBuf, Arc<DynamicImage>)>;
// clone() が参照カウントのインクリメントのみになる
```

**効果:** メモリ・CPU 大幅削減
**複雑度:** 低

---

### 2. encoded_chunks の Arc 化

**現状の問題:**
- app.rs で writer に渡す度に `Vec<Vec<u8>>` をクローン

**改善案:**
```rust
pub encoded_chunks: Arc<Vec<Vec<u8>>>,
```

**効果:** 大画像で効果大
**複雑度:** 低

---

### 3. as_raw().clone() の削減

**現状の問題:**
```rust
// kgp.rs:140-142
(v.as_raw().clone(), 24)  // ピクセルデータ全コピー
```

**改善案:**
- 借用で処理できる場合は Cow 使用
- Zlib 圧縮時は避けられないが、非圧縮時は借用可能

**効果:** メモリ削減
**複雑度:** 中

---

### 4. Tile サムネイルキャッシュ

**現状の問題:**
- ページ移動毎に全タイルを再デコード・リサイズ
- 同じ画像が異なるページで再処理される

**改善案:**
```rust
// worker.rs に追加
type ThumbnailCache = LruCache<(PathBuf, u32, u32), Arc<RgbaImage>>;
```

**効果:** Tile モードの体感改善
**複雑度:** 中

---

## Tier 2: 中インパクト

### 5. キャッシュ検索の HashMap 化

**現状の問題:**
```rust
// app.rs:428,533,649,771,843
render_cache.iter().find(|r| r.path == path && ...)  // O(n)
```

**改善案:**
```rust
type CacheKey = (PathBuf, (u32, u32), FitMode);
render_cache: HashMap<CacheKey, RenderedImage>,
// + LRU 順序管理用の VecDeque
```

**効果:** O(n) → O(1)
**複雑度:** 低

---

### 6. Tile 合成の並列化

**現状の問題:**
```rust
// worker.rs:307-409
for (i, path) in paths.iter().enumerate() {
    // 逐次デコード
}
```

**改善案:**
```rust
use rayon::prelude::*;
let thumbnails: Vec<_> = paths.par_iter()
    .map(|path| decode_and_resize(path))
    .collect();
```

**効果:** マルチコアで 3-5 倍高速化
**複雑度:** 中（rayon 依存追加）

---

### 7. Tile 高速フィルタ

**現状の問題:**
- `thumbnail()` はデフォルトで Lanczos3 を使用（高品質だが遅い）

**改善案:**
```rust
// config に追加
tile_filter: FilterType,  // Nearest, Triangle, etc.

// worker.rs
img.resize(w, h, self.config.tile_filter)
```

**効果:** CPU 削減（品質とのトレードオフ）
**複雑度:** 低

---

## Tier 3: 小改善

### 8. terminal::size() 呼び出し統合

**現状の問題:**
- main.rs で 5 箇所呼び出し (L140, 180, 322, 339, 384)

**改善案:**
- ループ冒頭で 1 回取得して変数に保持

**効果:** 微小
**複雑度:** 低

---

### 9. base64 encode_to_vec 使用

**現状の問題:**
```rust
// kgp.rs:154
base64_simd::STANDARD.encode_to_string(&data).into_bytes()
```

**改善案:**
```rust
base64_simd::STANDARD.encode_to_vec(&data)
```

**効果:** 微小（String 経由の変換を削減）
**複雑度:** 低

---

### 10. status キャッシュキー保持

**現状の問題:**
- 毎 tick で render_cache を線形走査して status を計算

**改善案:**
- 直近のキャッシュキーを保持して、変更がなければ再計算スキップ

**効果:** 微小
**複雑度:** 低

---

## 推奨実装順序

### Phase 1: 低コスト高リターン
1. #1 DynamicImage Arc 化
2. #2 encoded_chunks Arc 化
3. #5 HashMap キャッシュ

### Phase 2: UX 改善
4. #4 Tile サムネイルキャッシュ
5. #7 Tile 高速フィルタ

### Phase 3: 細かい改善
6. #8, #9, #10 の小改善

### 保留
- #3: 実装複雑で効果限定的
- #6: rayon 依存追加の是非を検討
