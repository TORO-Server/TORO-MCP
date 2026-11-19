# 設計メモ

利用者向けの概要は [README.md](../README.md) にあります。この文書は、受け付ける URL と認証、管理用のツールの決まり、機能ごとのソースの場所、安全のための決まりをまとめた、ただ 1 つの詳しい文書です。

## 受け付ける URL

| パス | 答え |
| --- | --- |
| `POST /` | JSON-RPC 2.0 (`initialize`、`tools/list`、`tools/call`、`ping` ほか)。通知には 202 |
| `GET /` | 発見用の JSON (ツールの一覧と、呼び出し元)。`Accept: text/event-stream` と `DELETE /` は 405 (サーバーから送り始めるものも、セッションも持たない) |
| `GET /` (ブラウザ) | Wiki の「AIクライアントの接続方法」へ 302。JSON を求める要求は除く |
| `/.well-known/oauth-protected-resource` | RFC 9728 の保護リソースの付随する情報。認可サーバーは TORO-Auth |
| `GET /healthz` | 稼働確認 |

認証は Bearer だけです。Cookie は見ません。

- トークンなし: 許す (読み取りのツールは使える)
- 無効・期限切れのトークン: 401 と `WWW-Authenticate` (クライアントが再認可できるように)
- auth に尋ねられなかったとき: 503。トークンが悪いとは限らないので、401 にして再認可させない

## ツール

コネクタの名前は、どのクライアントでも `TORO Server` です。ツールの一覧と説明の正本は `assets/mcp/tools.json` です。

- ツールを足す: `assets/mcp/tools.json` に定義を、`src/mcp/tools/` に実行を書き、`tools/mod.rs` の `call` に 1 行足す
- パスの正規化と権限の最終判断は TORO-API が行う。このサービスでは、保存の前に「ログイン済みか」「書ける権限か」だけを見る

### 管理用のツール (Admin 以上だけ)

荒らし対策 (CrimeBuster) と Discord のツールです。ツールの一覧と説明の正本は `assets/mcp/tools_admin.json`、実行は `src/mcp/tools/admin.rs` です。**いまの Discord のロールが `ADMIN_ROLE_IDS` のどれかで、トークンの scope に `mcp` がある (または scope の無い、本人が発行したトークンの) 人にだけ** `tools/list` に出て、呼ばれても、ここで断ります。`CRIMEBUSTER_CONNECTOR_URL` が無いとき、または `ADMIN_ROLE_IDS` が未設定か空のときは、Admin にも出しません (`ADMIN_ROLE_IDS` に既定の値は無く、起動は続けて匿名と Wiki のツールだけが使えます)。


- **ロールは、auth の確認の結果 (`roles_source` が `live`) だけを信じる。** auth は `/introspect` のたびに、トークンの値ではなく、いまの Discord のロールを返す (60 秒だけ覚える)。OAuth のアクセストークン (ロールが空) も 90 日トークン (ロールが古くなる) も、同じに扱え、ロールを外せば、最長 60 秒で使えなくなる。auth がロールを確かめられていないとき (`token` / `unavailable`、または古い auth) は、Admin と認めない。
- **OAuth のトークンは、同意した scope の中だけ。** 読むだけに同意したトークンでは、Admin の人でも管理用のツールは使えない。`mcp` を求める同意画面には、Admin 以上の人が何を許すことになるかが出る (TORO-Auth)。
- **コネクタには、共有の秘密ではなく、呼び出した利用者のトークンをそのまま渡す** (OAuth 2.0)。コネクタ (CrimeBuster) が auth に尋ねて、Admin かを自分で確かめ、トークンの持ち主を `audit.jsonl` に残す。ここの判断は、ツールを見せるかどうかだけ。Discord のトークンは、このサービスには無い (crime-buster が DiscordSRV の設定から読む)。
- **URL に入れる値は、検査してから。** 事件の ID は `[A-Za-z0-9_.-]` だけ (`..` を含めない)、プレイヤー名は `[A-Za-z0-9_.]` の 32 文字まで。
- **利用者が書いた文章を返すツールは、先頭に「信頼できない入力」の注意を付ける。** AI が、発言や Discord の本文の中の指示に従わないようにするため。
- コネクタへの 1 回の待ちは 30 秒です (`src/mcp/tools/admin.rs` の `DEFAULT_TIMEOUT`)。撮影は、始める・取るの 2 回に分けている。

## 機能ごとの置き場所

| 機能 | ソース | 試験 |
| --- | --- | --- |
| 振り分けと共通の上限 | `src/routes.rs` | `tests/hosts.rs`、`tests/security.rs` |
| MCP の受け口 | `src/mcp/transport.rs` | `tests/mcp.rs` |
| ツール | `src/mcp/tools/` | `tests/mcp.rs` |
| 保護リソースの付随する情報 | `src/mcp/metadata.rs` | `tests/hosts.rs` |
| トークンの確認 (auth の `/introspect`) | `src/identity.rs` | `tests/security.rs` |
| api への問い合わせ | `src/api.rs` | `tests/mcp.rs` |
| 設定、URL | `src/config.rs`、`src/site.rs` | `tests/hosts.rs` |
| アイコン | `src/static_files.rs` | `tests/mcp.rs` |

## セキュリティの考え方

- **JWT を自分では検証しない。** 署名の鍵はこのサービスに無い。トークンの有効性は auth だけが決める
- **Bearer だけ。** Cookie を見ないので、ブラウザからの CSRF は成り立たない
- **無効なトークンを匿名として扱わない。** 401 を返して再認可を促す。auth の障害は 503 で、再認可させない
- **scope を守る。** Wiki の変更には `write` か `mcp`、管理用のツールには `mcp` が要る。利用者のトークンは保存のときだけ api に渡す
- **利用者が書いた文章は、信頼できない入力として返す。** プレイヤーの発言、Discord の本文、Wiki の本文には、先頭に注意を付ける
- **内部の URL を見せない。** TORO-API や CrimeBuster につながらなかった理由はログにだけ残す
- **画像は埋め込む。** `serverInfo.icons` は小さな `data:` URI 1 つだけ (外部の画像を読めないクライアントがある)
- **応答は `no-store`。** 利用者ごとに違う応答を 途中の中継でためさせない (アイコンだけが例外)
- 同時に処理する数 (256)、ボディの大きさ (2 MiB)、ボディを読み切るまでの時間 (30 秒) に上限がある。稼働確認 (`/healthz`) は、同時処理の枠を使わない

試験は `tests/security.rs`。
