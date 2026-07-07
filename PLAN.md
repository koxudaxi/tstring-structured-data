# 調査レポート & 改善プラン: クリティカルバグ / リファクタリング / 最適化

調査日: 2026-07-07
対象: リポジトリ全体(Rust パーサ 3 クレート、tstring-core-rs、PyO3 バインディング 2 層、Python パッケージ 3 つ、scripts/)

調査方法: 領域ごとに独立レビューを実施し、バグ候補はコンパイル・実行して再現確認したものを **confirmed**、コードリーディングのみのものを **plausible** と表記。

---

## サマリ

| 分類 | 件数 | 特に重要なもの |
|------|------|----------------|
| Critical バグ | 6 | プロセス abort(スタックオーバーフロー)、YAML パニック、TOML の入力黙殺、YAML 大整数の float 化、O(n²) DoS ×2 |
| Major バグ | 12 | JSON キー順序破壊、Decimal 精度黙殺、**約 5,600 行のテストが一度もコンパイルされていない**、TOML 重複キー受理 ほか |
| Minor バグ | 15+ | エラーメッセージ・スペック適合の細部 |
| リファクタリング | 大規模な重複が 3 系統 | Python 値ディスパッチ ×6 コピー、パーサカーソル機構 ×2 コピー、Python パッケージ ×3 コピー |
| 最適化 | 8+ | TOML リテラル 3 回パース、per-call の import/getattr、フラット化ストリームの多重コピー |

横断的なテーマ(修正方針に影響):

1. **text 出力と data 出力の不一致**が繰り返し発生している(JSON キー順序、YAML float→int、YAML 大整数、重複キー)。「同じテンプレートから text と data は同じ意味論を持つ」を不変条件としてテストで固定すべき。
2. **バグの多くが重複コードの中に住んでいる**。Python 値ディスパッチは 6 箇所に複製されており、1 つのバグ修正に 6 箇所の変更が要る。リファクタリング(Phase 3)は品質対策そのもの。
3. **テストの空洞化**。json/toml/yaml 3 クレートの lib.rs 内 `#[cfg(test)]`(計約 5,600 行)は `[lib] test = false` + 依存不足で一度もコンパイル・実行されていない。カバレッジがあるように見える領域に critical バグが実在した。

---

## Phase 1 — Critical バグ修正(プロセス死・データ破壊・DoS)

### 1.1 再帰深度制限の欠如 → スタックオーバーフローでプロセス abort 【confirmed / 全バックエンド】

- パーサ: `json-tstring-rs/src/lib.rs:243,336,394`、`toml-tstring-rs/src/lib.rs:446,575,607`、`yaml-tstring-rs/src/lib.rs`(`parse_inline_value` → `parse_flow_*` → 再帰、ブロックネストも同様)
- バインディング側の Python 値変換にも同じ問題: `tstring-pyo3-bindings/src/{json.rs:353-558, toml.rs:417-572,914-1089, yaml.rs:595-752,1861-1993}` の `normalize_value` / `render_python_value` / `materialize_python_value`
- 再現: `"[".repeat(200_000)` のテンプレート、または `a = []; a.append(a); render_json(t"{a}")` → `fatal runtime error: stack overflow`(exit 134)。**スタックオーバーフローは panic として捕捉できず、Python インタープリタごと死ぬ。**
- 対応:
  - 全再帰パーサ・レンダラ・正規化関数に深度カウンタを導入し、上限超過で `TemplateParseError` / `UnrepresentableValueError` を返す(CPython の `json` は `RecursionError`、循環は `ValueError: Circular reference detected` を出す — 同等の挙動に)。
  - Python コンテナ変換には深度制限に加えて **循環検出**(訪問中コンテナの id セット)を入れる。
  - 深度制限は共有定数として tstring-core-rs に置く。

### 1.2 YAML: 末尾改行なしの `---` でスライス範囲パニック 【confirmed】

- `rust/yaml-tstring-rs/src/lib.rs:3479-3488,3526`(`split_stream` / `build_fragment`)
- 再現: `t"---"`, `t"--- "`, `t"a: 1\n---"` → `slice index starts at 4 but ends at 3` パニック(Python 側には PanicException として漏れる)。
- 対応: `current_start` が `items.len()` を超えないようクランプし、`start > end` のフラグメントは空ドキュメント or パースエラーに。回帰テスト追加。

### 1.3 TOML v1.1: インラインテーブル内のベアリテラルが改行を飲み込み、後続エントリを黙殺 【confirmed】

- `rust/toml-tstring-rs/src/lib.rs:965-1001,528-556`(`parse_literal` / `starts_value_terminator`)
- 再現: `t = { a = 1\nb = 2 }` → エラーにならず data は `{t: {a: 1}}`、**`b = 2` が無言で消える**。`materialize_value_source` が複数行ソースを `value = 1\nb = 2` という「正しい 2 文ドキュメント」に包んで再パースするため。format 出力と parse 結果も食い違う。
- 対応: `inline_table_depth > 0` の終端集合に `\n` を追加(期待挙動は「カンマ欠落のパースエラー」)。`materialize_value_source` に「ソースが単一値として消費しきれたか」の検証を追加。

### 1.4 YAML data 出力: i64 超の Python 整数が float(または inf)に化ける 【confirmed】

- `rust/tstring-pyo3-bindings/src/yaml.rs:606-616`(dict キー経路 688-716 も)
- 再現: `render_yaml(t"{2**63}")` → 期待 `9223372036854775808`(int)、実際 `9.223372036854776e18`(float)。400 桁だと `inf`。text 出力は正確な桁を出すので **text と data が矛盾**。JSON バックエンドは `arbitrary_precision` で正しく処理しており、TOML は正しくエラーになる。
- 対応: saphyr の scalar パースに頼らず、i64 に収まらない整数は JSON 経路と同様に文字列→`builtins.int` で正確に構築する。

### 1.5 O(n²) DoS ×2 【confirmed / 実測済み】

1. **YAML** `rust/yaml-tstring-rs/src/lib.rs:555-576`(`line_has_mapping_key_at`): プローブごとに `Vec<StreamItem>` 全体を clone して投機パース。実測: フラットな 2,000 行マッピングで 4.7 秒、8,000 行で 87 秒(debug)。
   - 対応: clone を廃し、`self.index` の save/restore か `&[StreamItem]` ベースの借用プローブに変更。さらにプローブ結果をキャッシュすればキーの二重パース(後述 4.2)も解消。
2. **JSON** `rust/json-tstring-rs/src/lib.rs:556,603-616`(`parse_promoted_string` / `starts_value_terminator`): 1 文字消費するたびに先読み空白列全体を再スキャン。実測: 空白 5 万個で 4.0 秒、10 万個で 18.2 秒(release)。TOML `parse_literal`(:969)にも同型あり。
   - 対応: 空白スキャン結果の位置をメモ化するか、終端判定を 1 パスの先読みに書き換え。共通カーソル化(Phase 3.2)と同時にやると 1 箇所の修正で済む。

### 1.6 Phase 1 の検証

- 各修正に再現入力をそのまま回帰テストとして追加(パーサ層は Rust テスト、バインディング層は `rust/yaml-pyo3-tests` / Python 側テストへ)。
- fuzz 的な深ネスト・巨大空白・循環参照のスモークテストを CI に常設。

---

## Phase 2 — Major 正確性バグ

### 2.1 テスト空洞化の解消(最優先: 他の修正の安全網)【confirmed】

- `json-tstring-rs/src/lib.rs:1164-2565`、`toml-tstring-rs/src/lib.rs:1520-3174`、`yaml-tstring-rs/src/lib.rs:3691-6796` の in-file `#[cfg(test)]` モジュール(計約 5,600 行)は、`[lib] test = false` かつ pyo3 / tstring-pyo3-bindings が依存に無いため **一度もコンパイルされていない**(強制ビルドで yaml だけで 243 コンパイルエラー)。実際に生きているテストは 3 クレート合計 27 個程度。
- 対応: これらのテストを依存が揃う `rust/yaml-pyo3-tests`(または新設の統合テストクレート)へ移設してコンパイル・実行可能にし、lib.rs から削除。CI で全クレートのテストターゲット数を検証するガードを追加。

### 2.2 JSON data 出力がキーをアルファベット順にソート 【confirmed】

- `serde_json` が `preserve_order` なしでビルドされているため(`rust/Cargo.toml:63`、`json.rs:232-251,417-434`)。`render_json(t'{"b":1,"a":2}')` → data `{'a':2,'b':1}`、text は元の順。`json.loads` とも Python dict 意味論とも不一致。
- 対応: `serde_json` に `preserve_order` フィーチャ追加(indexmap は toml 経由で既にツリー内にある)。

### 2.3 `__float__` を持つ任意オブジェクト(Decimal/Fraction 等)の黙殺的損失変換 【confirmed / 3 バックエンド共通】

- `extract::<f64>()` が `Decimal("1.100000000000000000001")` → `1.1` を成功させる(`json.rs:388,489`、`toml.rs:460,957`、`yaml.rs:617,696,1881,1937`)。`json.dumps` は Decimal で TypeError。
- 対応: `extract::<f64>()` を `PyFloat` の厳密 downcast に変更し、その他は `UnrepresentableValueError`。Phase 3.1 の共通ディスパッチ導入と同時に修正すれば 1 箇所で済む。

### 2.4 int サブクラスの `__str__` 経由の型混乱・テキスト注入 【confirmed】

- `tstring-pyo3-bindings/src/lib.rs:217-225`(`exact_integer_string`)が `str(value)` を信用。`__str__` が `"[1, 2]"` を返す int サブクラスで、JSON は配列として構造化され、YAML は生文字列がドキュメントに無検証で埋め込まれる(`"1\nevil: true"` で構造破壊)。
- 対応: CPython の json エンコーダ同様、`int.__repr__` / PyLong の桁を直接使う。

### 2.5 YAML text 出力: コンテナ内 float が int として描画 【confirmed】

- `yaml.rs:1714-1715`(`render_owned_scalar`)が `f64::Display` を使い `1.0` → `1`。`render_yaml_text(t"cfg: { {'x': 1.0}}")` → `x: 1`(再ロードで int 化)。data 出力は正しく `1.0` で、text/data 型不一致。
- 対応: 直接スカラー経路と同じ `finite_float_string` を使う。

### 2.6 JSON: `1e999` が `Finite(inf)` として不変条件破壊 【confirmed】

- `json-tstring-rs/src/lib.rs:1143-1162` + `tstring-core-rs/src/lib.rs:386-392`。debug ビルドでは `debug_assert` パニック、release では `NormalizedFloat::Finite(inf)` が下流に流れる(`PosInf` バリアントが存在するのに)。
- 対応: parse 後に `is_finite()` を検査して `PosInf`/`NegInf` に振り分けるか範囲エラーに。

### 2.7 TOML プロファイル適合の穴 【confirmed】

- v1.0 の秒省略ゲートが小文字 `t`・スペース区切りをすり抜け(`toml-tstring-rs/src/lib.rs:1038-1046`): `2024-01-02t03:04` / `2024-01-02 03:04` が V1_0 で受理される。→ デリミタ判定を `T`/`t`/space に拡張。
- 重複キー・重複テーブルヘッダを `check_template` が受理し format までする(spec ではハードエラー、lib.rs:1120 に意図的先送りのコメントあり)。→ post-parse 検証を実装するか、少なくとも `check_template` のドキュメント/命名を実態に合わせる。判断が要るため要オーナー確認。

### 2.8 YAML パーサの spec 逸脱 【confirmed】

- 重複タグ/アンカーが黙って上書き(`yaml-tstring-rs/src/lib.rs:388-410`): `&a1 &a2 x` で `a1` 消失 → 下流の `*a1` エイリアスが壊れる。`wrap_decorators`(3271-3285)にエラーの意図が既にあるので、parse 時に「複数タグ/アンカー禁止」エラーへ。
- ブロックスカラーのインデント指示子 `0` を受理し(spec は 1–9)、後続の兄弟行を全部飲み込む(:1800-1817)。→ `|0` をパースエラーに。

### 2.9 JSON: RFC 8259 外の Unicode 空白を受理 【confirmed】

- `char::is_whitespace()` 使用(`json-tstring-rs/src/lib.rs:229-233,603-616`)。NBSP や U+2028 を含むテンプレートが `rfc8259` として валид 扱い(過剰受理のみ、出力注入はなし)。→ space/tab/LF/CR のみに限定。

---

## Phase 3 — リファクタリング(重複解消 = バグ増殖の根本対策)

### 3.1 バインディング層: Python スカラーディスパッチの一本化(~700 行削減)

- None/bool/int/float/str/datetime/list/dict/tuple の判定ラダーが **6 回複製**されている(`json.rs` ×2、`toml.rs` ×2、`yaml.rs` ×2 + キーゲート)。Phase 2 のバグ #2.3/#2.4 や surrogates 問題はすべてこの中に住んでおり、現状 1 バグ = 6 箇所修正。
- 対応: `Bound<PyAny> → 中間スカラー enum` の単一ビジタを作り、3 バックエンドはそれを消費する形に。**Phase 2 のバインディング系修正はこのビジタ導入とセットで行うのが最も効率的。**
- あわせて: `PreparedFormattedSlot` + `formatted_text` メモ化(3 クレートに逐語コピー)、`type_name()`/`expression()` ヘルパの共通化。

### 3.2 パーサ層: カーソル機構を tstring-core-rs へ(~250 行/クレート削減)

- `current / mark / span_from / advance / consume_char / flush_buffer / consume_interpolation` 等が JsonParser と TomlParser にコピペ(YAML も同型)。O(n²) 修正(1.5)や span バグ(`flush_buffer` が全チャンクに `SourceSpan::point(0,0)` を付ける問題)も 1 箇所で直せるようになる。
- stringly-typed 判定(`current_kind() == "eof"` の文字列比較、`role: String`/`style: String` のヒープ確保)を enum 化。

### 3.3 YAML パーサ内部の重複

- `parse_block_sequence` / `parse_compact_sequence`(~75 行、差分はループ終了比較のみ)、`parse_block_mapping` / `parse_compact_mapping`、`parse_mapping_value` 内の 15 行ブロック ×4 を統合。2.8 の decorator 修正は現状 2 箇所に当てる必要があるため、統合を先行または同時に。
- 「コメントを EOL まで読み飛ばす」ループが約 10 箇所 → `skip_comment_line()` に抽出。
- `normalize_representation_value/_key`、`normalize_core_tagged_scalar_value/_key` のペア重複解消。

### 3.4 Python パッケージ 3 つの統合(~250 行削減)

- `_runtime.py` は 3 パッケージで ~95% 同一、`_errors.py`/`_slots.py` はバイト単位で同一(しかも自パッケージから import されておらず、カバレッジ 100% 要件のためだけに存在)。
- 拡張契約チェックが import 時と `_bind_extension()` で二重実行。プロファイル解決も `_profiles.py` と各パッケージで逐語重複。
- **デフォルトプロファイル "1.1" が 3 箇所に独立ハードコード**(`toml_tstring/_runtime.py:77`、`tstring_bindings/_profiles.py:10`、`python-bindings/src/lib.rs:293` の pyo3 デフォルト)— 将来のデフォルト変更で `toml_tstring.render_data` と `tstring_bindings.render_toml` が黙って食い違うリスク。→ tstring_core に共有ファクトリを置き、デフォルトは単一定義に。
- `__all__` の不整合(`JsonTemplate` 等の欠落、順序の差)も同時に揃える。

### 3.5 デッドコード削除

- `python-bindings/Cargo.toml` の `pythonize`(ワークスペース全体で 0 使用)。
- `json-tstring-rs` の `parse_string(quoted: bool)` — 常に `true` で呼ばれ `!quoted` 分岐は死んでいる。
- tstring_core の純 Python パイプライン(`_tokens.py`/`_nodes.py`/`_diagnostics.py` 等)は本番経路で未使用(Rust 拡張が全て担う)。非推奨 `_render.render_with_backend` の扱いと合わせて、削除 or 「実験的」明記を判断。`_render.py:52` の結果を捨てる `tokenize_template` 呼び出しと :72 の文法ミス("require" → "requires")は即修正。
- `scripts/update_docs_version.py`: 対象パターンが README/docs のどこにもマッチしない恒久 no-op(下記 5.1 と合わせて削除 or 復旧を判断)。

---

## Phase 4 — 最適化

ホットパス(render 呼び出しごと)から順に:

1. **`ensure_template` が呼び出しごとに `string.templatelib` を再 import**(`tstring-pyo3-bindings/src/lib.rs:159-174`)→ `GILOnceCell` で型をキャッシュ。
2. **data 出力の整数が毎回 `import builtins` → `getattr("int")` → 文字列パース**(`python-bindings/src/lib.rs:473-476,514-517`)→ i64/i128 は `IntoPyObject` の高速パス、big int のみ文字列経路。datetime モジュールも同様にキャッシュ(:561,614)。
3. **text と data の二重トラバース**(`json.rs:198-230` の `execute_interpolation` が常に両方計算、YAML も同様)→ 呼び出し側の要求に応じた単一トラバースに。
4. **TOML ベアリテラルの 3 回パース**(`toml-tstring-rs/src/lib.rs:1079-1093`: char スキャン → `"value = {src}"` の再パース → `toml::from_str` + Table clone)→ 1 回のパース結果を再利用。
5. **ParseCache の O(n) LRU touch**(`python-bindings/src/lib.rs:107-112`、ヒットごとに mutex 下で VecDeque 線形走査)→ HashMap+世代カウンタ等に。パース中に `py.detach()` / `allow_threads` が無く GIL がスレッドを直列化している点も改善余地。
6. **フラット化ストリームの多重コピー**: TOML は `template.flatten()` を 1 パースで 2 回実行(:1099,195)、YAML は flatten + fragment `to_vec` + (修正前は)プローブ clone で ~10x 入力サイズを 3 回実体化。`from_parts`/`tokenize` の不要 clone(`tstring-core-rs/src/lib.rs:514-529`)も消費 move に。
7. 小物: `escape_json_fragment` の `to_string` + `remove(0)`(O(n) シフト)→ 直接エスケープループ、YAML エスケープの per-char String 確保 → `char`/`&'static str` 返し。

計測: `rust/tstring-pyo3-bindings/benches/render_paths.rs` が既にあるので、Phase 4 の各項目は before/after を criterion で記録する。

---

## Phase 5 — Minor バグ・スクリプト・テスト補強

### 5.1 リリース/CI スクリプト

- `scripts/update_docs_version.py:31-40,52`: `gh release list` の出力を無検証で `PATTERN.sub` に流す(タグ `v0.4.0` → `json-tstring==v0.4.0`、リリース無し → `==null`)。しかも対象パターンは現 docs にマッチせず恒久 no-op。→ `manage_versions.normalize_tag` 相当の正規化 + 検証を入れるか、スクリプトと CI ステップごと削除。
- `scripts/sync_conformance_vendor.py:117`: `extractall` に `filter="data"` なし(3.14 未満で tar path-traversal)。yaml LICENSE が `ref="main"` から取得されマニフェストのピンとズレる。tempdir 未クリーンアップ。

### 5.2 バックエンド間の挙動不一致(方針を決めて統一)

- 文字列フラグメント中の `None`: TOML は明示拒否、JSON/YAML は `str()` で `"prefix None"` を出す(`json.rs:314-351` ほか)。
- 重複キー `t'{"a":1,"a":2}'`: text は両方残し data は last-wins(JSON/YAML)。エラーにするか、最低限ドキュメント化。
- lone surrogate 文字列: 実因(UnicodeEncodeError)を握りつぶし「str は非対応」と誤報(`json.rs:361→443` ほか)。

### 5.3 その他 minor

- `tstring_core/_conformance.py:127-128`: `parents[3]` はソースチェックアウト前提で、wheel からは FileNotFoundError(conformance/ は wheel に入らない)。テスト専用なら wheel から除外。
- YAML: 非 verbatim タグの `:` 打ち切り(:418)、`...` 始まり行の過剰拒否(:3356-3370)、quoted 折返しのタブをインデント算入(:1520-1524)、DEL/C1 制御文字の未エスケープ再出力(:2472 ほか、plausible)、`from_items` の Eof 無し OOB(:201)。
- TOML: エラーメッセージのプロファイル固定 "TOML v1.0"(:403-408)、コメント内制御文字の誤診断(:295-305)、直接 `TomlParser` 使用時の裸 `\r` 受理(:1021-1036)。
- `examples/_display.py:44-47`: 正当な null ルート(`data=None`)を TypeError 扱い。`zip(strict=False)` → `strict=True`。

### 5.4 テストカバレッジの穴(全パッケージ共通)

- PEP 750 テンプレート連結(`t"..." + t"..."`)が render に到達するテストが 0 件。
- 空テンプレート `t""` の明示テストなし。
- `backend-e2e-tests` は名前に反して PyO3 境界を一切越えない — Phase 2 のバインディング系修正(キー順序、Decimal、big int)の e2e テストを追加。

---

## 推奨着手順序

1. **Phase 2.1(テスト移設)を最初に**行い、既存の 5,600 行のテスト資産を生き返らせて安全網を作る。
2. **Phase 1** の critical 6 件(各修正 + 再現入力の回帰テスト)。1.5 の O(n²) は Phase 3.2 のカーソル共通化と同時でも良い。
3. **Phase 3.1(共通ビジタ)と Phase 2 のバインディング系バグ(2.2–2.5)をセットで**実施 — 6 箇所修正を 1 箇所修正に変えてから直す。
4. Phase 2 の残り(パーサ spec 適合)→ Phase 3 残り → Phase 4(ベンチ計測付き)→ Phase 5。
5. 2.7 の TOML 重複キー検証は意図的先送りのコメントがあるため、実装前にメンテナ判断を仰ぐ。

破壊的変更の注意: 2.2(キー順序)、2.3(Decimal 拒否)、1.3(TOML エラー化)、2.8(YAML エラー化)は「今まで通っていた入力がエラーになる/出力が変わる」変更。CHANGELOG で明示し、必要ならマイナーバージョンを上げる。
