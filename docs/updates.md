# アプリの自動アップデート

リリースビルドはプロセス起動時に1回、更新を確認します。新版があれば、「Install and restart」でインストール・再起動するか、「Later」で延期できます。起動時の確認では、最新版の場合や通信に失敗した場合は通知しません。インストール失敗時はエラーを表示します。

Settingsには「Check for updates」、現在のバージョン、更新の進捗・結果を表示します。確認・インストールの重複実行は防止します。デバッグビルドでは更新しません。

Tauriのupdater・dialogプラグインを使用します。更新確認先は`https://github.com/t-hamano/english-input-assistant/releases/latest/download/latest.json`です。

ダウンロードしたファイルは、`src-tauri/tauri.conf.json`の公開鍵で署名を検証します。検証に成功したファイルだけをインストールします。

## 署名鍵

GitHub Actionsでは、リポジトリのSecret `TAURI_SIGNING_PRIVATE_KEY`に登録した秘密鍵で署名します。公開鍵は`src-tauri/tauri.conf.json`の`plugins.updater.pubkey`に設定しています。

今後のリリースでも同じ鍵を使います。新しいリリースの公開鍵だけを変更すると、既存のインストールからそのリリースの署名を検証できなくなります。秘密鍵はコミットせず、リリースの添付ファイルにも含めません。

## リリース手順

`package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`のバージョンを揃え、対応する`vX.Y.Z`タグをpushします。

リリースワークフローはWindows x64のNSISインストーラーとmacOS Apple Siliconの配布ファイル・署名を生成し、`latest.json`とともにドラフトリリースにアップロードします。共通の更新情報ファイルに両OSの情報を残すため、ビルドは順番に実行します。両方のビルドが成功した後に公開します。

`-`を含むタグはプレリリース扱いとなり、安定版の更新確認先からは除外されます。

アップデーター未搭載の既存アプリには、初回だけ対応版を手動インストールします。それ以降の新版から自動更新できます。

初回の対応版を公開するまでは、更新確認先が404を返す場合があります。手動確認ではエラーを表示し、起動時の確認では通知しません。

## 動作確認

- `npm run build`と`cargo test --manifest-path src-tauri/Cargo.toml --lib`を実行します。
- リリース版をインストールした状態で、署名付きの新版を公開してアプリを再起動します。Settingsが閉じていても確認画面が出ること、「Later」でアプリが起動したままになること、「Install and restart」で新版に更新されることを確認します。
- 最新版の場合、オフラインの場合、別の更新確認が実行中の場合に手動確認します。
- ダウンロード進捗と、ダウンロード失敗後の再試行を確認します。改ざんされたファイルや別の鍵の署名では、既存アプリを置き換えずに失敗することを確認します。
- 広く配布する前に、WindowsとmacOSの両方で更新を確認します。
