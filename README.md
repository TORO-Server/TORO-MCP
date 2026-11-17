# toro-mcp

TORO Server の AI コネクタ (AI クライアントがつなぐ Remote MCP サーバー) です。Rust で書かれています。

利用者がつないだ AI に、Wiki の原稿の読み書き、Minecraft サーバーの稼働状態、利用者の権限の確認といった道具 (ツール) を渡します。Admin 以上の人には、荒らし対策と Discord の道具も出します。

このサービスは画面を持たず、秘密も持ちません。トークンの確認は [TORO-Auth](https://github.com/TORO-Server/TORO-Auth) に、Wiki と稼働状態のデータは [TORO-API](https://github.com/TORO-Server/TORO-API) に頼みます。

## 動かしてみる

```bash
cargo test   # 試験。偽の TORO-Auth と偽の TORO-API を自動で立てるので、秘密もネットワークも要りません
cargo run    # 動かす。TORO-Auth と TORO-API に届く URL を環境変数で渡します
```

設定の名前と既定値の正本は [.env.example](.env.example) です。ツールの一覧と説明の正本は [assets/mcp/tools.json](assets/mcp/tools.json) と [assets/mcp/tools_admin.json](assets/mcp/tools_admin.json) です。

## 関連文書

- 受け付ける URL、認証、管理用のツールの決まり、ソースの場所、安全のための決まり: [docs/design.md](docs/design.md)
- サービス全体の構成: [ServerImage-TORO の docs/architecture.md](https://github.com/TORO-Server/ServerImage-TORO/blob/main/docs/architecture.md)
- 文章の書き方: [WRITING-STYLE.md](https://github.com/TORO-Server/TORO-Website/blob/main/docs/WRITING-STYLE.md)
