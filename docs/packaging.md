# Windows 打包

Chronolume 2.1 使用 Tauri 2 构建原生 Release EXE、当前用户级 NSIS 安装器和便携 ZIP。Windows 继续使用 `com.gaos6e.codexusage`，以保持安装升级和 WebView 偏好连续。打包前先执行标准验证：

```powershell
npm ci
npm run check:versions
npm run typecheck
npm run lint
npm test
npm run build

Set-Location src-tauri
cargo fmt --check
cargo check --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test
Set-Location ..

npm run tauri:build:windows
.\scripts\build-portable.ps1
.\scripts\smoke-distributions.ps1
```

主要产物：

```text
src-tauri\target\release\chronolume.exe
src-tauri\target\release\bundle\nsis\Chronolume_<版本>_x64-setup.exe
src-tauri\target\release\bundle\portable\Chronolume-<版本>-windows-x64-portable.zip
```

便携 ZIP 包含同一个 Release EXE、README、项目 `LICENSE` 和由实际锁文件生成的 `THIRD_PARTY_LICENSES.txt`，不附带分析数据库或用户数据。三个性能 benchmark 由显式 Cargo feature 管理，仅供开发验证，安装器和便携包都不得包含它们；smoke 会审计安装目录只能出现 `chronolume.exe` 与 `uninstall.exe`。`build-portable.ps1` 会在压缩前重新执行许可证审计；旧的手写 `THIRD_PARTY_NOTICES.md` 不再是输入。安装器当前未签名，Windows SmartScreen 可能在建立签名信誉前显示警告。运行时目录由 Windows 平台路径解析，不依赖源码仓库位置。

## 2.1.10 验证记录（2026-10-07）

应用源码提交 `542b9ab4c945498e89324c3278af757ef656aeb2`（`v2.1.10`）。版本一致性、前端类型检查/Lint/40 项测试/生产构建及 Rust 格式/检查/Clippy/89 项测试通过；发布提交的 Windows 与 macOS CI 均通过。

| 产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| `chronolume.exe` | 10,460,672 | `3B8DDE73EA7CF75A8E9AA2C5636EA07236473A77A6057B05DCEB69607A287B50` |
| `Chronolume_2.1.10_x64-setup.exe` | 3,244,684 | `C51E36A1761EA95D1705D7344FFBDFB977DC98DC9DA82050E4FBC9405C332AF2` |
| `Chronolume-2.1.10-windows-x64-portable.zip` | 4,377,901 | `6C7E29B248E04C66CBB631B06C22DA6D922258AE2EE52F5C597C898B8159BF0A` |

许可证清单按本版锁文件重新生成，并在最终 NSIS 打包前更新。静默安装退出码为 0；原安装目录 `D:\software\Chronolume` 中的应用版本为 2.1.10，应用与卸载器两个 EXE 的审计通过。安装版与便携版分别在 328.10 ms、321.51 ms 创建窗口；这些数字只证明启动检查，不代表查询性能变化。两种分发的版本、哈希、ZIP 条目与结构化 smoke 记录均已由暂存脚本校验。

## 2.1.9 验证记录（2026-10-04）

完成 Release EXE、NSIS 安装器与便携 ZIP 构建，保留原有自定义安装目录及分析数据库，更新桌面快捷方式。`smoke-distributions.ps1` 可通过 `-InstallDirectory` 指定既有目录；默认仍使用当前用户的标准安装目录。启动前核对可执行文件版本，报告同时记录版本与 SHA-256。

| 产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| `chronolume.exe` | 10,413,056 | `F410251521AC219926222E50451C4C50BA04CE71CB80BF6FAA91CFED2C7EA6A2` |
| `Chronolume_2.1.9_x64-setup.exe` | 3,222,283 | `AB18ECB6A4E05B1B8ED1BEDE9F9393256EAB04C36A6A7DD7DB71391B52CCC290` |
| `Chronolume-2.1.9-windows-x64-portable.zip` | 4,355,222 | `882A49A7FD7EA5C82B47F26240AA9042BEA196C4D985E8E044CECA9F0D8CCC56` |

NSIS 静默安装退出码为 0；安装目录仅含应用与卸载器两个 EXE。最终检查中，安装版在 326.42 ms、便携解压版在 284.46 ms 创建窗口；该数字仅表示窗口出现，不代表数据加载完成或性能提升。两者均确认版本为 2.1.9，验证报告由发布暂存脚本校验。

NSIS 会将应用中的 Tauri 分发标记从 `UNK` 改为 `NSS`，因此安装目录 EXE 与独立 EXE 的原始哈希不同。还原这三个标记字节后，二者完整内容一致。许可证文件重生成成功，未修改应用依赖版本。

## 2.1.0 验证记录

以下结果来自本分支完成的 Windows Release/NSIS 构建、许可证重生成、便携打包与 `smoke-distributions.ps1`，没有沿用 2.0.2 历史数字。

| 产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| `chronolume.exe` | 10,357,248 | `47BAEB1FC7AE07B3F984D847F8F65B795120147EDC3A8CE3B9ABFB78BBB24337` |
| `Chronolume_2.1.0_x64-setup.exe` | 3,208,213 | `3043A7164390A41D5BDD94E83493B803107D339602682A66408B5C82C7A8D9A5` |
| `Chronolume-2.1.0-windows-x64-portable.zip` | 4,330,319 | `949C852F2A928D155E02DE70DD8F069552031508B479B8B62A1335AB71FEFFFB` |

NSIS 静默安装退出码为 0；安装目录二进制审计通过；最终复跑中安装版在 107.59 ms 创建窗口，便携 ZIP 解压版在 92.63 ms 创建窗口。二者均由 smoke 脚本按自身 PID 正常关窗，ZIP 解压目录已安全清除；结构化 smoke JSON 又由发布暂存脚本读取并校验。`THIRD_PARTY_LICENSES.txt` 为 752,490 字节，SHA-256 为 `F470B4F3D31C1B6BE3C93526AD77726E7545A3A64C1F2BD915802F028E21E947`。

## 2.0.2 最终产物（2026-07-11）

| 产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| `chronolume.exe` | 10,357,248 | `C871BADC7EDE00710CCDF33A1236CA29E251B9BEF899F8DC5AEDD966B5CAB693` |
| `Chronolume_2.0.2_x64-setup.exe` | 4,184,150 | `5C0BE952CB871DFE6556090092E560D224FAAD87518B0D45ABD8919535D25A18` |
| `Chronolume-2.0.2-windows-x64-portable.zip` | 4,222,150 | `C90EB5F292D7D020544B6FCE99DC469A5C8EBA9F952363335B3096508F470DBC` |

本轮最终打包由 Tauri/NSIS 正常完成。NSIS 静默安装退出码为 0；安装版在 108.05 ms 创建窗口，便携 ZIP 解压版在 97.40 ms 创建窗口。二者均由测试脚本按自身 PID 正常关窗，ZIP 解压目录已安全清除。桌面 `Chronolume.lnk` 指向本轮安装的 `%LOCALAPPDATA%\Chronolume\chronolume.exe`。

真实 2.0.0 数据迁移前后均为 1,801 个会话、82,569 条保留事件、1,804 个来源检查点、28 条模型价格和 2 项应用设置；迁移后的 SQLite `quick_check` 为 `ok`。旧程序目录、旧快捷方式和旧卸载注册项已按绝对路径定向清理，`~/.codex`、用户导出和 benchmark 未被删除。
