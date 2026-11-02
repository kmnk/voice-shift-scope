# packaging/licenses

`scripts/third_party_licenses.py` が、配布物の `THIRD-PARTY-LICENSES.txt` を作るときに使うライセンスのファイル。どれも元のファイルを手を加えずに写した。

## crates/

ライセンスのファイルを同梱しない crate の分。ディレクトリの名前は `<crate の名前>-<版>` で、script は crate のソースにライセンスのファイルが無いときにここを見る。crate の版を上げてここに無くなると、script が止まるので、その版の元のファイルを置き直す。

| ディレクトリ | 元 |
| --- | --- |
| `alloc-stdlib-0.3.0` | https://github.com/dropbox/rust-alloc-no-stdlib/blob/0a81fd6928ea/LICENSE |
| `dasp_sample-0.11.0` | https://github.com/rustaudio/sample/blob/97c3bb9b2363/LICENSE-MIT と `LICENSE-APACHE` |
| `tauri-plugin-2.7.2` | https://github.com/tauri-apps/tauri/blob/59b63ab500da/LICENSE-MIT と `LICENSE-APACHE-2.0` |
| `webview2-com-0.39.1`、`webview2-com-sys-0.39.1` | https://github.com/wravery/webview2-rs/blob/edc2caf88617/LICENSE |
| `webview2-com-macros-0.8.1` | https://github.com/wravery/webview2-rs/blob/dffa41a8a46d/LICENSE |
| `selectors-0.38.0` | 元のリポジトリ（https://github.com/servo/stylo の 572ecba2d160）に無いので、https://www.mozilla.org/media/MPL/2.0/index.txt |

commit は、各 crate の `.cargo_vcs_info.json` が指すもの。

## webview2-loader/

`webview2-com-sys` が exe に組み込む Microsoft の WebView2 の loader の分。NuGet の `Microsoft.Web.WebView2` 1.0.3650.58（https://www.nuget.org/packages/Microsoft.Web.WebView2/1.0.3650.58 ）の `LICENSE.txt` と `NOTICE.txt`。
