# AfterburnerWebMonitor

> **当前版本 v2.6.1** —— v2.0 起用 Rust 完全重写：内存 22 MB → **12 MB**，体积 15.3 MB → **500 KB 级**，无窗口静默运行，专属图标。
> 无需安装 Python，单个 exe 即用。源码见 [`rust/`](rust/)，发布包见 [Releases](https://github.com/wyunki462-stack/afterburner-web-monitor/releases/latest)。

把 **MSI Afterburner** / **HWiNFO64** 的硬件监控数据变成手机可看的实时网页。

用旧手机当电脑的硬件监控副屏 —— 打游戏时随时看温度和帧数。

<p align="center">
  <img src="https://img.shields.io/badge/Rust-1.7x-orange" alt="Rust">
  <img src="https://img.shields.io/badge/Platform-Windows-lightgrey" alt="Platform">
  <img src="https://img.shields.io/badge/License-MIT-green" alt="License">
</p>

---

## 核心特点

**小飞机模式：你在 Afterburner 的 OSD 上勾选什么，网页就显示什么。**

想改显示内容？只需在 Afterburner 里改勾选，网页自动跟随，**不用改任何代码**。

**HWiNFO 模式：补全 Afterburner 读不到的读数。**

Afterburner 的监控表里没有 CPU 温度（Tctl/Tdie）、CPU 功耗、GPU 热点温度、显存结温这些项。
换用 HWiNFO 数据源就能拿到，而且可以在网页上自己勾选要显示哪些。

- **三档数据源**，网页上随时切换：小飞机 / HWiNFO 全显示 / HWiNFO 自选
- 单个 exe，绿色免安装，无运行时依赖
- 无窗口后台运行，托盘图标控制
- 手机、平板只要能连 WiFi 就能看
- 内置开机自启设置（四个开关，真正写入 Windows 启动项）

---

## 工作原理

程序只读三个共享内存，完全不干扰 Afterburner / RTSS / HWiNFO 的正常工作：

| 数据 | 来源 | 说明 |
|---|---|---|
| 温度 / 占用 / 频率 / 功耗 | `MAHMSharedMemory` | Afterburner 的监控共享内存 |
| 温度 / 功耗 / 风扇 / 结温 | `Global\HWiNFO_SENS_SM2` | HWiNFO64 的传感器共享内存 |
| 帧率 / 帧时间 / 1% low | `RTSSSharedMemoryV2` | RTSS 的帧率统计共享内存 |

关键点：Afterburner 的监控项里，每个数据都带一个 OSD 标志位
（`MAHM_SHARED_MEMORY_ENTRY_FLAG_SHOW_IN_OSD`）。
小飞机模式下本程序只读取勾选了这个标志的项，从而实现"OSD 勾什么就读什么"。

HWiNFO 的共享内存默认是**关闭**的，需要先在 HWiNFO 里开启
（`Settings → Shared Memory Support`）。本程序提供**一键开启**。

---

## 前置要求

| 要求 | 说明 |
|---|---|
| **MSI Afterburner** | 小飞机模式必须安装并运行（传感器数据来源） |
| **HWiNFO64** | HWiNFO 模式必须安装并运行（需**管理员权限** + **Sensors-only** 模式） |
| **RTSS** | 想看帧数必须有（Afterburner 启动时会自动带起它） |
| Windows | 已测试 Windows 10 / 11 |

程序会自动在常见安装位置搜索这些程序，找不到可在 `config.json` 手动指定。

### 配置 Afterburner（小飞机模式的关键步骤）

1. 打开 MSI Afterburner
2. 点齿轮图标 → **监控** 选项卡
3. 在"活动硬件监控图表"里**选中**你想看的项目
4. 对每个选中项，勾选下方的 **「在 OSD 上显示」**
5. 点「应用」

网页会自动跟随你的勾选。

### 配置 HWiNFO（HWiNFO 模式的关键步骤）

1. 打开 HWiNFO64 —— **必须"以管理员身份运行"**
2. 启动时选 **Sensors-only**（只开传感器窗口）
3. 进入 `Settings` → 勾选 **Shared Memory Support** → 确定
4. 回网页把数据源切到 **HWiNFO 全显示** 或 **HWiNFO 自选**

> 懒得手动配？网页上直接点「一键开启共享内存 / 启动 HWiNFO」，
> 程序会帮你写配置并以管理员权限拉起。然后在设置面板里把
> **HWiNFO 开机自启**打开，以后开机就会自动带上它（会弹一次 UAC）。

---

## 使用

### 方式一：直接运行 exe（推荐普通用户）

1. 从 [Releases](../../releases) 下载 `AfterburnerWebMonitor-v2.6.1-x64-portable.zip` 并解压
2. 双击 `AfterburnerWebMonitor.exe`
3. 弹出提示框会显示手机访问地址，例如 `http://192.168.4.9:8777`
4. 手机连同一 WiFi，浏览器打开该地址

### 方式二：从源码构建（开发者）

见 [`rust/README.md`](rust/README.md)。

---

## 功能说明

### 网页界面

- **数据源切换**：硬件监控标题旁的三个按钮 —— 小飞机 / HWiNFO 全显示 / HWiNFO 自选
- **HWiNFO 自选弹窗**：搜索框 + 分组复选（GPU / CPU / 内存 / 硬盘 / 其他）+ 全选 / 全不选 / 智能默认
- **帧数卡片**：当前游戏名、实时帧数、1% low / 0.1% low / 帧时间、60 秒帧数折线图
- **传感器卡片**：按 GPU / CPU / 内存 / 其他 自动分组
- **转屏按钮**：一键旋转 90 度，适配手机横屏摆放
- **设置面板**：四个开机自启开关（本程序 / Afterburner / RTSS / HWiNFO）+ 一键启动对应程序
  - 每个开关下面会显示**实际状态**（「✓ 已在开机启动项」），避免"开了但没生效"
  - 写入 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，关掉开关即删除

### 三档数据源的取舍

| 档位 | 说明 | 本机实测项数 |
|---|---|---|
| **小飞机** | 读 Afterburner 的 OSD 勾选项，改勾选网页自动跟随 | 约 10~13 |
| **HWiNFO 全显示** | HWiNFO 共享内存里的全部读数，信息最全也最杂 | 约 410 |
| **HWiNFO 自选** | 自己挑，默认走"智能筛选"（过滤掉 Limit、Maximum 之类的噪声项） | 默认 75 |

自选项按 `英文原始标签 + 原始传感器名` 作为稳定标识，所以**换界面语言也不会丢失你的勾选**。

### 托盘菜单

- 打开网页
- 启动 Afterburner
- 启动 RTSS（帧数）—— 一键以管理员权限启动
- 打开日志
- 退出

---

## 配置文件

首次运行自动生成 `config.json`：

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
| `port` | 网页端口，冲突可改（默认 8777） |
| `ab_path` / `rtss_path` / `hwinfo_path` | 对应程序的 exe 路径，留空自动探测 |
| `source` | 数据源：`ab`（小飞机）/ `hwfull`（HWiNFO 全显示）/ `hwcustom`（HWiNFO 自选） |
| `hwinfo_selected` | HWiNFO 自选项列表；`null` = 跟随智能默认，`[]` = 一项都不显示 |
| `auto_start_ab` / `auto_start_rtss` / `auto_start_hwinfo` | 启动本程序时是否自动拉起对应程序 |
| `auto_start_app` | 是否开机自启本程序 |

---

## 常见问题

**Q: 网页显示"Afterburner 未运行"？**

打开 MSI Afterburner 即可，或在设置面板里点「启动 Afterburner」。

**Q: 网页显示"没有勾选任何 OSD 监控项"？**

打开 Afterburner → 设置 → 监控，逐个勾选想看的项目，并勾上「在 OSD 上显示」。

**Q: 看不到帧数？**

帧数来自 RTSS，检查两点：

1. **RTSS 在运行吗？** 看任务栏右下角有没有图标
2. **有 3D 程序在跑吗？** 帧率是游戏渲染出来的 —— 桌面、浏览器、视频都**没有**帧率可测

**Q: CPU 温度显示不出来？**

Afterburner 的监控列表里通常**没有** CPU 温度这一项 —— 这是它的老问题。
把数据源切到 **HWiNFO**，CPU 温度（Tctl/Tdie）和 CPU 功耗都能读到。

**Q: HWiNFO 显示"未运行"或读不到数据？**

三个必要条件，缺一不可：

1. HWiNFO 必须**以管理员身份运行**（否则读不到传感器，也会启动失败）
2. 必须开 **Shared Memory Support**（`Settings` 里勾选）—— 网页上有一键开启
3. 传感器窗口要保持打开（HWiNFO 关掉传感器窗口，共享内存就停了）

**Q: HWiNFO 用着用着突然没数据了？**

免费版的共享内存连续运行 **12 小时**后会失效，重启 HWiNFO 即可。
本程序检测到长时间无数据会自动尝试重启 HWiNFO，但免费版的这个限制无法绕过。

**Q: 打开「开机自启」开关但开机后没启动？**

两个常见原因：

1. **RTSS / HWiNFO 需要管理员权限** —— 开机时会弹 UAC，不点「是」就不启动。
   想免弹窗，用任务计划程序配置「最高权限运行 + 登录时触发」，
   然后把网页设置里的对应开关**关掉**，避免启动两次。
2. 开关下面那行会显示实际状态。如果写的是「启动项未生效」，
   把开关关掉再打开一次即可重试。

> 从 v2.6.0 及更早版本升级上来的，程序会自动清理旧版 Python 版留下的
> `LHMViewer_AB` / `LHMViewer_RTSS` 启动项，不需要手动删。

**Q: 手机打不开？**

- 确认手机和电脑连同一 WiFi
- Windows 防火墙首次会弹窗，选**允许访问**
- 或手动放行：

```bash
netsh advfirewall firewall add rule name="AfterburnerWebMonitor" dir=in action=allow protocol=TCP localport=8777
```

**Q: 端口被占用？**

说明已有一个实例在运行。任务管理器结束旧的 `AfterburnerWebMonitor.exe`，或改 `config.json` 的 `port`。

---

## 为什么要读 OSD 标志位

MSI Afterburner 的监控配置里可能有上百个数据项（本机实测 122 项），
但用户真正关心的通常只有十来个。

与其在程序里写死"我要读 GPU 温度、CPU 占用……"，不如直接复用 Afterburner 里
用户已经配置好的那套选择 —— 也就是 OSD 显示项。

好处：

- 用户改需求不用改代码
- 不需要再维护一份"我关心哪些指标"的配置
- 和用户实际在游戏里看到的信息完全一致

HWiNFO 没有这样的"用户选择"标志位，所以走了另一条路：
**全量解析 + 关键词过滤 + 自动分组**，再让用户在自己勾选（自选档）。

---

## 限制

- 仅支持 Windows（依赖 Windows 共享内存 API）
- 帧率数据依赖 RTSS，无游戏运行时无帧率
- HWiNFO 免费版共享内存有 **12 小时**连续运行限制
- HWiNFO 必须管理员权限运行，会弹一次 UAC
- 开机自启里的 RTSS / HWiNFO 同样需要管理员权限，从启动项拉起时会弹 UAC（无法绕过）

---

## License

MIT

---

## 致谢

- [MSI Afterburner](https://www.msi.com/Landing/afterburner) / [RivaTuner Statistics Server](https://www.guru3d.com/files-details/rtss-rivatuner-statistics-server-download.html) / [HWiNFO64](https://www.hwinfo.com/) —— 数据源
- [windows-sys](https://github.com/microsoft/windows-rs) —— Windows API 绑定
