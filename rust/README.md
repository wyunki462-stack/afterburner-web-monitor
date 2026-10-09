# AfterburnerWebMonitor

把 **MSI Afterburner** 的硬件监控数据变成手机可看的网页 —— 手机连同一 WiFi，浏览器打开就能实时看 CPU/GPU 温度、占用率、帧数等。

> 当前版本 **v2.4.1**。v2.0 起用 **Rust** 重写，相比 v1.0（Python）内存占用从 ~50 MB 降到 **~12 MB**，体积从 19 MB 降到 **460 KB**，且无任何运行时依赖。

## 特性

- **零配置数据源** — 直接读 Afterburner 的 OSD 勾选项，在 Afterburner 里改勾选，网页自动跟随
- **极低占用** — 单文件 exe，内存约 5 MB，体积约 1 MB
- **无运行时依赖** — 不需要安装 Python / .NET / 任何运行库
- **帧率监控** — 从 RTSS 读取实时帧率、帧时间、1% Low、0.1% Low，含 60 秒折线图
- **响应式网页** — 手机竖屏友好，深色主题
- **一键启动** — 可从网页启动 Afterburner / RTSS（RTSS 需 UAC 提权）

## 快速开始

1. 从 [Releases](../../releases) 下载 `AfterburnerWebMonitor-v2.4.1-x64-portable.zip`
2. 解压，双击 `AfterburnerWebMonitor.exe`
3. 弹出提示框会显示手机访问地址，例如 `http://192.168.4.9:8777`
4. 手机连同一 WiFi，浏览器打开该地址

### 前置条件

- **MSI Afterburner** — 必须运行，且在设置里勾选要在 OSD 显示的监控项
  （`设置 → 监控`，勾选「在 OSD 上显示」）
- **RivaTuner Statistics Server (RTSS)** — 可选，用于读取帧率。不启动则无帧数显示

## 工作原理

```
MSI Afterburner  ──(命名共享内存 MAHMSharedMemory)──┐
                                                     ├──> AfterburnerWebMonitor ──> HTTP :8777 ──> 手机浏览器
RTSS             ──(命名共享内存 RTSSSharedMemoryV2)─┘
```

- **MAHMSharedMemory**：Afterburner 暴露的传感器数据。每个条目带 `dwFlags`，
  其中 `MAHM_SHARED_MEMORY_ENTRY_FLAG_SHOW_IN_OSD`(bit0) 表示该项已在 OSD 显示。
  本程序只读取**已勾选 OSD** 的项 —— 所以改 Afterburner 的勾选，网页自动跟随。
- **RTSSSharedMemoryV2**：RTSS 暴露的帧率数据。签名 `0xDEAD` 表示 RTSS 未运行。

  两个容易踩的坑（v2.4.1 已修）：
  - 头部的 `dwAppArrSize` **存的是条目数量（MAX_APPS = 256），不是字节数**
    —— 有些 SDK 注释写成"数组大小"很容易误导，按字节去除会得到 0 个槽位，帧率就永远读不出来
  - 时间字段 `dwTime0/dwTime1` 单位是**毫秒**，`fps = frames * 1000 / (time1 - time0)`

  另外，RTSS 的应用表里会同时列出浏览器、办公软件等被 hook 的进程，
  它们的帧率有时比游戏还高。所以**不能简单取帧率最高的那个当游戏**——
  本程序以 Afterburner 报告的帧率为锚点，取应用表里最接近的进程。

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

## 配置

首次运行会在 exe 同目录生成 `config.json`：

```json
{
  "port": 8777,
  "ab_path": "",
  "rtss_path": "",
  "auto_start_ab": true,
  "auto_start_rtss": true
}
```

- `port` — HTTP 端口，默认 8777
- `ab_path` / `rtss_path` — 留空则自动探测常见安装路径
- `auto_start_*` — 启动时是否自动拉起 Afterburner / RTSS

## HTTP 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/` | 监控页面 |
| GET | `/api/data` | 当前传感器数据 + 帧率 |
| GET | `/api/fps_history` | 最近 60 秒帧率历史 |
| GET | `/api/settings` | 读取配置 |
| POST | `/api/settings` | 更新配置 |
| POST | `/api/start/ab` | 启动 Afterburner |
| POST | `/api/start/rtss` | 启动 RTSS（UAC 提权） |

## 防火墙

首次运行 Windows 会询问是否允许网络访问，选**允许**。

若手机访问不了，手动放行端口：

```cmd
netsh advfirewall firewall add rule name="AfterburnerWebMonitor" dir=in action=allow protocol=TCP localport=8777
```

## License

MIT

## 致谢

- [MSI Afterburner](https://www.msi.com/Landing/afterburner) / [RTSS](https://www.guru3d.com/files-details/rtss-rivatuner-statistics-server-download.html) —— 数据源
- [windows-sys](https://github.com/microsoft/windows-rs) —— Windows API 绑定
