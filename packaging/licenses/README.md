# packaging/licenses

License files that `scripts/third_party_licenses.py` uses when it makes `THIRD-PARTY-LICENSES.txt` for the release packages. All are copied from the originals without changes.

## crates/

For crates that do not ship a license file. The directories are named `<crate name>-<version>`, and the script looks here when a crate's source has no license file. If a crate's version is raised and is no longer here, the script stops; put the original files for that version here.

| Directory | Source |
| --- | --- |
| `alloc-stdlib-0.3.0` | https://github.com/dropbox/rust-alloc-no-stdlib/blob/0a81fd6928ea/LICENSE |
| `dasp_sample-0.11.0` | https://github.com/rustaudio/sample/blob/97c3bb9b2363/LICENSE-MIT and `LICENSE-APACHE` |
| `tauri-plugin-2.7.2` | https://github.com/tauri-apps/tauri/blob/59b63ab500da/LICENSE-MIT and `LICENSE-APACHE-2.0` |
| `webview2-com-0.39.1`, `webview2-com-sys-0.39.1` | https://github.com/wravery/webview2-rs/blob/edc2caf88617/LICENSE |
| `webview2-com-macros-0.8.1` | https://github.com/wravery/webview2-rs/blob/dffa41a8a46d/LICENSE |
| `selectors-0.38.0` | Not in the original repository (572ecba2d160 of https://github.com/servo/stylo ), so https://www.mozilla.org/media/MPL/2.0/index.txt |

The commits are the ones each crate's `.cargo_vcs_info.json` points to.

## webview2-loader/

For the Microsoft WebView2 loader that `webview2-com-sys` builds into the exe: `LICENSE.txt` and `NOTICE.txt` from the NuGet package `Microsoft.Web.WebView2` 1.0.3650.58 (https://www.nuget.org/packages/Microsoft.Web.WebView2/1.0.3650.58 ).
