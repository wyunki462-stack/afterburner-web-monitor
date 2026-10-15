# AfterburnerWebMonitor

把 **MSI Afterburner** / **HWiNFO64** 的硬件监控数据变成手机可看的网页 —— 手机连同一 WiFi，浏览器打开就能实时看 CPU/GPU 温度、占用率、帧数等。

> 当前版本 **v2.6.1**。v2.0 起用 **Rust** 重写，相比 v1.0（Python）内存占用从 ~50 MB 降到 **~12 MB**，体积从 19 MB 降到 **500 KB 级**，且无任何运行时依赖。

## 特性

- **三档数据源** — 小飞机（Afterburner OSD 勾选项）/ HWiNFO 全显示 / HWiNFO 自选，网页上随时切换
- **零配置数据源（小飞机模式）** — 直接读 Afterburner 的 OSD 勾选项，在 Afterburner 里改勾选，网页自动跟随
- **补全 Afterburner 读不到的读数** — 换 HWiNFO 后可拿到 CPU 温度（Tctl/Tdie）、CPU 功耗、GPU 热点温度、显存结温等
- **极低占用** — 单文件 exe，内存约 12 MB
- **无运行时依赖** — 不需要安装 Python / .NET / 任何运行库
- **帧率监控** — 从 RTSS 读取实时帧率、帧时间、1% Low、0.1% Low，含 60 秒折线图
- **响应式网页** — 手机竖屏友好，深色主题
- **一键启动** — 可从网页启动 Afterburner / RTSS / HWiNFO（需 UAC 提权）

## 快速开始

1. 从 [Releases](../../releases) 下载 `AfterburnerWebMonitor-v2.6.0-x64-portable.zip`
2. 解压，双击 `AfterburnerWebMonitor.exe`
3. 弹出提示框会显示手机访问地址，例如 `http://192.168.4.9:8777`
4. 手机连同一 WiFi，浏览器打开该地址

### 前置条件

- **MSI Afterburner** — 小飞机模式必须运行，且在设置里勾选要在 OSD 显示的监控项
  （`设置 → 监控`，勾选「在 OSD 上显示」）
- **HWiNFO64** — HWiNFO 模式必须运行（**管理员身份** + **Sensors-only** 模式）；程序可一键帮你配置
- **RivaTuner Statistics Server (RTSS)** — 可选，用于读取帧率。不启动则无帧数显示

## 工作原理

```
MSI Afterburner  ──(命名共享内存 MAHMSharedMemory)────────┐
                                                            │
HWiNFO64         ──(命名共享内存 Global\HWiNFO_SENS_SM2)───┼──> AfterburnerWebMonitor ──> HTTP :8777 ──> 手机浏览器
                                                            │
RTSS             ──(命名共享内存 RTSSSharedMemoryV2)───────┘
```

### MAHMSharedMemory

Afterburner 暴露的传感器数据。每个条目带 `dwFlags`，其中
`MAHM_SHARED_MEMORY_ENTRY_FLAG_SHOW_IN_OSD`(bit0) 表示该项已在 OSD 显示。
本程序只读取**已勾选 OSD** 的项 —— 所以改 Afterburner 的勾选，网页自动跟随。

条目内偏移：`flags@1312` / `gpu@1316` / `srcId@1320`（entrySize = 1324）。
`FLT_MAX (3.4e38)` 表示该数据当前不可用（如没游戏时的帧率），必须跳过。

### HWiNFO_SENS_SM2

HWiNFO 传感器窗口暴露的共享内存。头部为 `sig / ver / rev / offset / size / count` 布局。

**8.54 实测的坑**：sensor 元素 392 字节、reading 元素 **460 字节**（尾部有填充）。
如果按"元素大小必须是结构体大小"去推偏移，`Value` 会被推到 288 —— 读出来全是 0。
有效字段仍在经典偏移上，直接固定：

| 字段 | 偏移 | 长度 |
|---|---|---|
| type | 0 | 4 |
| sensorIdx | 4 | 4 |
| readingID | 8 | 4 |
| szLabelOrig | 12 | 128 |
| szLabelUser | 140 | 128 |
| szUnit | 268 | 16 |
| **Value (double)** | **284** | 8 |
| Min | 292 | 8 |
| Max | 300 | 8 |
| Avg | 308 | 8 |
| szLabelUTF8 | 316 | 128 |
| szUnitUTF8 | 444 | 16 |

另两个必踩的坑：

- **过滤要用英文原始标签**（`szLabelOrig`）。`Performance Limit` / `Thermal Limit`
  这类条目在中文界面下 `szLabelUser` 是中文译名，按中文匹配会漏掉一片噪声项。
  显示时反过来用 UTF-8 标签（`szLabelUTF8`）。
- **HWiNFO 必须管理员权限**运行（否则 `WinError 740`），且启动时会弹"选择启动模式"对话框
  —— 无人点击约 30 秒后自动退出。所以程序里的一键启动是**提权 + Sensors-only 参数**一起做的。

### RTSSSharedMemoryV2

RTSS 暴露的帧率数据。签名 `0xDEAD` 表示 RTSS 未运行。

两个容易踩的坑（v2.4.1 已修）：

- 头部的 `dwAppArrSize` **存的是条目数量（MAX_APPS = 256），不是字节数**
  —— 有些 SDK 注释写成"数组大小"很容易误导，按字节去除会得到 0 个槽位，帧率就永远读不出来
- 时间字段 `dwTime0/dwTime1` 单位是**毫秒**，`fps = frames * 1000 / (time1 - time0)`

槽位偏移：`pid@0` `name@4`(260) `flags@264` `time0@268` `time1@272` `frames@276`。

另外，RTSS 的应用表里会同时列出浏览器、办公软件等被 hook 的进程，
它们的帧率有时比游戏还高。所以**不能简单取帧率最高的那个当游戏**——
本程序以 Afterburner 报告的帧率为锚点，取应用表里最接近的进程。

> 注意：Afterburner 的帧率跟随"当前被统计的 3D 应用"，游戏切到后台会跳到别的进程（OSD 同行为，属预期）。

## 开机自启（`src/autostart.rs`）

设置面板里的四个自启开关，各自对应 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
下的**一条独立启动项**（`config.json` 存意愿，本模块负责落到注册表）：

| 开关 | 注册表值名 |
|---|---|
| AfterburnerWebMonitor | `AfterburnerWebMonitor`（带 `--no-dialog`） |
| MSI Afterburner | `AfterburnerWebMonitor_AB` |
| RTSS | `AfterburnerWebMonitor_RTSS` |
| HWiNFO | `AfterburnerWebMonitor_HWiNFO` |

语义：

- 勾选且程序存在 → 写入启动项（路径含空格时加引号）
- 勾选但程序没找到 → **保持原样**，不清掉用户可能手动配好的项
- 未勾选 → 删除启动项

启动时与每次保存设置时都会同步一次（配置即真相）。

> **历史坑**：这几个开关在 v2.6.0 及以前是**摆设** —— 只写 `config.json`，
> 完全没有注册表代码。v1.0（Python 版，当时叫 LHMViewer）其实写过启动项，
> 值名是 `LHMViewer_AB` / `LHMViewer_RTSS`，Rust 重写时这段实现被弄丢了。
> v2.6.1 用新名字接管，并在启动时清掉旧的 `LHMViewer_*`，
> 否则开机时 Afterburner / RTSS 会被拉起两次。

> ⚠️ RTSS 与 HWiNFO64 的 manifest 要求管理员权限，从启动项拉起时 Windows 会弹 UAC。
> 这是它们自身的设计，程序无法绕过 —— 想免掉弹窗需要用任务计划程序配置「最高权限运行」。

## 数据源三档切换

网页"硬件监控"标题旁有分段按钮：

| 档位 | 说明 | 典型项数（本机实测） |
|---|---|---|
| **小飞机** | 读 Afterburner 的 OSD 勾选项 | 约 10~13 |
| **HWiNFO 全显示** | HWiNFO 共享内存里的全部读数 | 约 410 |
| **HWiNFO 自选** | 在弹窗里自己勾选（分组 + 搜索 + 智能默认） | 默认 75 |

实现上是一次解析、三次投影：

```
read_raw()  ──>  RawSet { rows }        // 全量解析（含英文/UTF-8 双标签、分组、噪声标记）
decorate()  ──>  消歧（重名读数加传感器前缀）
arrange()   ──>  排序 + 截断（PER_GROUP = 24）
to_mahm()   ──>  MahmData
```

自选项的稳定 key = `英文原始标签 + \u{1} + 原始传感器名`，所以换语言界面也不会丢失选择。
`hwinfo_selected = null` 表示跟随「智能默认」，`[]` 表示一项都不显示。

## HWiNFO 自动配置

HWiNFO 默认不开共享内存，且必须管理员运行。程序提供**一键配置**：

1. **开启共享内存** —— 写 `HWiNFO64.INI` 的 `[Settings] SensorsSM=1`（便携版）
   或注册表 `HKCU\Software\HWiNFO64\SensorsSM`（安装版）。必须在 HWiNFO **关闭**时写。
2. **提权启动** —— 以管理员身份拉起 HWiNFO，等待共享内存出现（最多 30 秒）。
3. **自愈** —— 运行期若连续 20 次读不到、且距上次成功超过 300 秒，自动重启 HWiNFO。

> **免费版限制**：HWiNFO 免费版共享内存连续跑满 **12 小时**会失效，需要重启 HWiNFO。
> 付费版无此限制。

## 从源码构建

需要 Rust 工具链 + MinGW-w64（GNU target）：

```bash
# 安装 Rust
winget install Rustlang.Rustup
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu

# 安装 MinGW-w64（提供链接器）
winget install BrechtSanders.WinLibs.POSIX.UCRT

# 构建
cargo build --release
# 产物: target/release/AfterburnerWebMonitor.exe
```

或直接运行 `build.bat`。

> 用户名含中文时，`ld` 可能因编码问题失败。`build.bat` 里把
> `CARGO_TARGET_DIR` 指到纯 ASCII 路径（如 `D:\abwm_target`）即可规避。

## 配置

首次运行会在 exe 同目录生成 `config.json`：

```json
{
  "port": 8777,
  "ab_path": "",
  "rtss_path": "",
  "hwinfo_path": "",
  "source": "ab",
  "hwinfo_selected": null,
  "auto_start_ab": true,
  "auto_start_rtss": true,
  "auto_start_hwinfo": false,
  "auto_start_app": true
}
```

| 字段 | 说明 |
|---|---|
| `port` | HTTP 端口，默认 8777 |
| `ab_path` / `rtss_path` / `hwinfo_path` | 留空则自动探测常见安装路径 |
| `source` | 数据源：`ab` / `hwfull` / `hwcustom`（旧值 `hwinfo` 自动映射为 `hwcustom`） |
| `hwinfo_selected` | HWiNFO 自选项的 key 列表；`null` = 跟随智能默认 |
| `auto_start_*` | 启动时是否自动拉起对应程序 |
| `auto_start_app` | 是否开机自启本程序 |

## HTTP 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/` | 监控页面 |
| GET | `/api/data` | 传感器数据 + 帧率（含 `source` / `active` / `ab_ok` / `hwinfo_ok`） |
| GET | `/api/fps_history` | 最近 60 秒帧率历史 |
| GET | `/api/settings` | 读取配置 |
| POST | `/api/settings` | 更新配置 |
| POST | `/api/source` | 切换数据源（`ab` / `hwfull` / `hwcustom`） |
| GET | `/api/hwinfo/list` | 列出 HWiNFO 全部读数 + 三档投影结果 |
| POST | `/api/hwinfo/selected` | 保存 HWiNFO 自选项 |
| POST | `/api/hwinfo/setup` | 一键开启共享内存 / 提权启动 HWiNFO |
| POST | `/api/start/ab` \| `/api/start/rtss` \| `/api/start/hwinfo` | 启动对应程序 |

## 防火墙

首次运行 Windows 会询问是否允许网络访问，选**允许**。

若手机访问不了，手动放行端口：

```cmd
netsh advfirewall firewall add rule name="AfterburnerWebMonitor" dir=in action=allow protocol=TCP localport=8777
```

## 版本历史

| 版本 | 变更 |
|---|---|
| **2.6.1** | 修复「开机自启」开关全是摆设 —— 补齐 `src/autostart.rs`，四个开关真正写入 `HKCU\...\Run`；新增 HWiNFO 开机自启开关；接管并清理旧版 `LHMViewer_*` 启动项 |
| **2.6.0** | 数据源改为三档（小飞机 / HWiNFO 全显示 / HWiNFO 自选），新增自选弹窗（搜索、分组、智能默认） |
| **2.5.0** | 新增 HWiNFO64 数据源；一键开启共享内存 + 提权启动；过滤从 425 项收敛到 75 项并自动分组 |
| 2.4.1 | 修复帧率读不到（RTSS 应用表长度字段解析错误 + 前后端字段不匹配） |
| 2.4.0 | 全新应用图标（仪表弧 + 火焰），内嵌 7 种尺寸 |
| 2.3.0 | 隐藏控制台黑窗口，纯托盘静默运行 |
| 2.2.0 | 修复「点确定后进程退出」导致手机打不开网页 |
| 2.1.0 | 修复「显示了全部 122 项而非勾选项」，补回温度/功耗配色 |
| 2.0.0 | 用 Rust 完全重写，内存 22MB → 12MB，体积降到 400KB 级 |
| 1.0.0 | Python 初版 |

## License

MIT

## 致谢

- [MSI Afterburner](https://www.msi.com/Landing/afterburner) / [RTSS](https://www.guru3d.com/files-details/rtss-rivatuner-statistics-server-download.html) / [HWiNFO64](https://www.hwinfo.com/) —— 数据源
- [windows-sys](https://github.com/microsoft/windows-rs) —— Windows API 绑定
