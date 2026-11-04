# toro-mcp

TORO Server の AI コネクタ (Remote MCP サーバー) です。Rust 製で、`https://mcp.torosaba.net/` で動きます。

## 役割

| URL | 役割 | ここにあるもの |
| --- | --- | --- |
| `https://auth.torosaba.net/` | だれなのかを確かめる | Discord ログイン、トークンの発行、OAuth 2.0 の認可 (TORO-Auth) |
| `https://api.torosaba.net/` | データを読み書きする | Wiki の原稿、稼働状態、OpenAPI の仕様書 (TORO-API) |
| `https://mcp.torosaba.net/` | AI クライアントがつなぐ | **このリポジトリ**。ブラウザで開くと Wiki の「AIクライアントの接続方法」へ移動する |

このサービスは画面を持たず、秘密も持ちません。トークンの確認は auth に、Wiki と稼働状態のデータは api に頼みます。

```
AI クライアント → Cloudflare → cloudflared ─→ toro-mcp:8080 ─┬─ toro-auth:8081 (POST /introspect: トークンの確認)
                                                             └─ toro-api:8080  (Wiki と稼働状態のデータ)
```

- どのホスト名で受けても、同じ MCP サーバーとして答えます (ホストごとの振り分けはありません)
- 構成: Cloudflare Tunnel の公開ホスト名 `mcp.torosaba.net` → `http://toro-mcp:8080`
- `AUTH_INTERNAL_URL` と `API_INTERNAL_URL` の先に、内部のネットワークで届く必要があります。どちらも公開しません

## 窓口

| パス | 答え |
| --- | --- |
| `POST /` | JSON-RPC 2.0 (`initialize`、`tools/list`、`tools/call`、`ping` ほか)。通知には 202 |
| `GET /` | 発見用の JSON (ツールの一覧と、呼び出し元)。`Accept: text/event-stream` と `DELETE /` は 405 (サーバーから送り始めるものも、セッションも持たない) |
| `GET /` (ブラウザ) | Wiki の「AIクライアントの接続方法」へ 302。JSON を求める要求は除く |
| `/.well-known/oauth-protected-resource` | RFC 9728 の保護リソースのメタデータ。認可サーバーは auth |
| `/favicon.ico`、`/apple-touch-icon.png`、`/icon-{64,192,512}.png` ほか | TORO のアイコン |
| `GET /healthz` | 稼働確認 |

認証は Bearer だけです。Cookie は見ません。

- トークンなし: 許す (読み取りのツールは使える)
- 無効・期限切れのトークン: 401 と `WWW-Authenticate` (クライアントが再認可できるように)
- auth に尋ねられなかったとき: 503。トークンが悪いとは限らないので、401 にして再認可させない

## ツール

コネクタの名前は、どのクライアントでも `TORO Server` です。定義は `assets/mcp/tools.json`。

| ツール | できること | 頼む先 |
| --- | --- | --- |
| `wiki_list_pages` | 原稿の一覧 (カテゴリで絞れる) | api `GET /docs/list` |
| `wiki_get_page` | 原稿と SHA (先頭に「信頼できない入力」の注意が付く) | api `GET /docs/raw` |
| `wiki_save_page` | 原稿の保存 (コミット)。編集権限と、`write` か `mcp` の scope が要る | api `POST /docs/save` (利用者の Bearer を渡す) |
| `wiki_search` | 原稿の全文検索 | api `GET /docs/search` |
| `server_get_status` | Minecraft サーバーの稼働状態 | api `GET /server/status` |
| `server_get_info` | 公式サービスの URL | 設定から作る |
| `auth_get_identity` | 呼び出している利用者と権限 | トークンの確認の結果 |

- ツールを足す: `assets/mcp/tools.json` に定義を、`src/mcp/tools/` に実行を書き、`tools/mod.rs` の `call` に 1 行足す
- パスの正規化と権限の最終判断は api が行う。MCP では、保存の前に「ログイン済みか」「書ける権限か」だけを見る

### 管理用のツール (Admin 以上だけ)

荒らし対策 (toro-guard) と Discord のツールです。定義は `assets/mcp/tools_admin.json`、実行は `src/mcp/tools/admin.rs`。**いまの Discord のロールが `ADMIN_ROLE_IDS` のどれかで、トークンの scope に `mcp` がある (または scope の無い、本人が発行したトークンの) 人にだけ** `tools/list` に出て、呼ばれても、ここで断ります。`GUARD_CONTROL_URL` が無いときは、Admin にも出しません。

| ツール | できること | 頼む先 (toro-guard の窓口) |
| --- | --- | --- |
| `guard_status` `guard_list_incidents` `guard_get_incident` `guard_get_image` | 状態・事件の一覧・報告文・証拠の画像 (MCP の `image` コンテンツ) | `GET /v1/status` `/v1/incidents` `/v1/incidents/{id}` `…/images/{name}` |
| `guard_report_incident` `guard_rollback` `guard_dismiss_incident` | 通報 / ロールバック (裏で動く) / 却下 | `POST /v1/incidents/{id}/report` `…/rollback` `…/dismiss` |
| `guard_player_activity` `guard_recent_chat` | 最近のブロックの変更・コマンド・発言 | `GET /v1/players/{name}/activity` `/v1/chat` |
| `guard_mute_player` `guard_kick_player` | ミュート / キック | `POST /v1/players/{name}/mute` `…/kick` |
| `guard_map_screenshot` `guard_get_screenshot` | BlueMap の撮影を始める (job) / 撮れた画像を取る | `POST /v1/screenshot` `GET /v1/screenshots/{job}` |
| `discord_search_tools` `discord_call_tool` | TORO の Discord ボットで、許可した操作 (読む・投稿・タグ付け) | `GET /v1/discord/tools` `POST /v1/discord/call` |

- **ロールは、auth の確認の結果 (`roles_source` が `live`) だけを信じる。** auth は `/introspect` のたびに、トークンの値ではなく、いまの Discord のロールを返す (60 秒だけ覚える)。OAuth のアクセストークン (ロールが空) も 90 日トークン (ロールが古くなる) も、同じに扱え、ロールを外せば、最長 60 秒で使えなくなる。auth がロールを確かめられていないとき (`token` / `unavailable`、または古い auth) は、Admin と認めない。
- **OAuth のトークンは、同意した scope の中だけ。** 読むだけに同意したトークンでは、Admin の人でも管理用のツールは使えない。`mcp` を求める同意画面には、Admin 以上の人が何を許すことになるかが出る (TORO-Auth)。
- **窓口には、共有の秘密ではなく、呼び出した利用者のトークンをそのまま渡す** (OAuth 2.0)。窓口 (toro-guard) が auth に尋ねて、Admin かを自分で確かめ、トークンの持ち主を `audit.jsonl` に残す。ここの判断は、ツールを見せるかどうかだけ。Discord のトークンは、このサービスには無い (toro-guard が DiscordSRV の設定から読む)。
- **URL に入れる値は、検査してから。** 事件の ID は `[A-Za-z0-9_.-]` だけ (`..` を含めない)、プレイヤー名は `[A-Za-z0-9_.]` の 32 文字まで。
- **利用者が書いた文章を返すツールは、先頭に「信頼できない入力」の注意を付ける。** AI が、発言や Discord の本文の中の指示に従わないようにするため。
- 窓口への 1 回の待ちは 30 秒 (Muse のコネクターの 1 回の呼び出しが 45 秒で切れるため)。撮影は、始める・取るの 2 回に分けている。
- つなぎ方 (Muse): toro-guard の `docs/MUSE.md`。

## 機能ごとの置き場所

| 機能 | ソース | 試験 |
| --- | --- | --- |
| 振り分けと共通の上限 | `src/routes.rs` | `tests/hosts.rs`、`tests/security.rs` |
| MCP の受け口 | `src/mcp/transport.rs` | `tests/mcp.rs` |
| ツール | `src/mcp/tools/` | `tests/mcp.rs` |
| 保護リソースのメタデータ | `src/mcp/metadata.rs` | `tests/hosts.rs` |
| トークンの確認 (auth の `/introspect`) | `src/identity.rs` | `tests/security.rs` |
| api への問い合わせ | `src/api.rs` | `tests/mcp.rs` |
| 設定、URL | `src/config.rs`、`src/site.rs` | `tests/hosts.rs` |
| アイコン | `src/static_files.rs` | `tests/mcp.rs` |

## 開発

```bash
cargo test
```

`tests/common/mod.rs` が偽の auth と偽の api を立てるので、試験に秘密やネットワークは要りません。

手元で動かすときは、auth と api を用意するか、`AUTH_INTERNAL_URL` と `API_INTERNAL_URL` に手元の URL を入れて `cargo run` します。

## 設定

`.env.example` を見てください。秘密の値はありません。既定は本番の値です。

| 環境変数 | 既定 | 意味 |
| --- | --- | --- |
| `AUTH_INTERNAL_URL` | `http://toro-auth:8081` | トークンの確認を頼む先 |
| `API_INTERNAL_URL` | `http://toro-api:8080` | Wiki と稼働状態のデータを頼む先 |
| `GUARD_CONTROL_URL` | (空) | 荒らし対策の窓口 (内部のネットワークだけ。例 `http://toro-guard:8090`)。空なら、管理用のツールは出さない |
| `ADMIN_ROLE_IDS` | Owner と Admin のロール ID | 管理用のツールを使える Discord のロール (カンマ区切り) |
| `PUBLIC_AUTH_URL` / `PUBLIC_API_URL` / `PUBLIC_MCP_URL` / `PUBLIC_WIKI_URL` | 本番の URL | 利用者に見せる URL |
| `CORS_ORIGINS` | `https://wiki.torosaba.net` | Cookie 付きで呼べる Origin (カンマ区切り) |
| `PORT` | `8080` | 待ち受けるポート |

**URL の正本は 1 か所です。** 別の場所に書き写さないでください。

- コード: `Config` (`src/config.rs`) だけが URL を持つ
- プログラム向け: `https://api.torosaba.net/` の JSON
- 人向け: Wiki の「サービスのURLと役割」のページ

## イメージ

GitHub Actions が、試験に通ったものだけを GHCR (`ghcr.io/toro-server/toro-mcp`) に置きます (linux/amd64)。タグ `v*` を打つとバージョンのタグが付きます。scratch の上で静的に動き、`HEALTHCHECK` はバイナリ自身 (`toro-mcp healthcheck`) が行います。

## セキュリティの考え方

- **JWT を自分では検証しない。** 署名の鍵はこのサービスに無い。トークンの有効性は auth だけが決める
- **Bearer だけ。** Cookie を見ないので、ブラウザからの CSRF は成り立たない
- **無効なトークンを匿名として扱わない。** 401 を返して再認可を促す。auth の障害は 503 で、再認可させない
- **scope を守る。** Wiki の変更には `write` か `mcp`、管理用のツールには `mcp` が要る。利用者のトークンは保存のときだけ api に渡す
- **利用者が書いた文章は、信頼できない入力として返す。** プレイヤーの発言、Discord の本文、Wiki の本文には、先頭に注意を付ける
- **内部の URL を見せない。** api や窓口につながらなかった理由はログにだけ残す
- **画像は埋め込む。** `serverInfo.icons` は小さな `data:` URI 1 つだけ (外部の画像を読めないクライアントがある)
- **応答は `no-store`。** 利用者ごとに違う応答を Cloudflare にためさせない (アイコンだけが例外)
- 同時に処理する数 (256)、ボディの大きさ (2 MiB)、ボディを読み切るまでの時間 (30 秒) に上限がある

試験は `tests/security.rs`。
