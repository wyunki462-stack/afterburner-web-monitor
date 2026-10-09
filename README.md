# AfterburnerWebMonitor

把 **MSI Afterburner** 的硬件监控数据变成手机可看的实时网页。

用旧手机当电脑的硬件监控副屏 —— 打游戏时随时看温度和帧数。

<p align="center">
  <img src="https://img.shields.io/badge/Python-3.8%2B-blue" alt="Python">
  <img src="https://img.shields.io/badge/Platform-Windows-lightgrey" alt="Platform">
  <img src="https://img.shields.io/badge/License-MIT-green" alt="License">
</p>

---

## 核心特点

**你在 Afterburner 的 OSD 上勾选什么，网页就显示什么。**

想改显示内容？只需在 Afterburner 里改勾选，网页自动跟随，**不用改任何代码**。

- 数据源直接用 Afterburner 的共享内存，无需额外安装监控程序
- 单个 exe，绿色免安装
- 无窗口后台运行，托盘图标控制
- 手机、平板只要能连 WiFi 就能看
- 内置开机自启设置

---

## 工作原理

程序只读两个共享内存，完全不干扰 Afterburner / RTSS 的正常工作：

| 数据 | 来源 | 说明 |
|---|---|---|
| 温度 / 占用 / 频率 / 功耗 | `MAHMSharedMemory` | Afterburner 的监控共享内存 |
| 帧率 / 帧时间 / 1% low | `RTSSSharedMemoryV2` | RTSS 的帧率统计共享内存 |

关键点：Afterburner 的监控项里，每个数据都带一个 OSD 标志位
（`MAHM_SHARED_MEMORY_ENTRY_FLAG_SHOW_IN_OSD`）。
本程序只读取勾选了这个标志的项，从而实现"OSD 勾什么就读什么"。

---

## 前置要求

| 要求 | 说明 |
|---|---|
| **MSI Afterburner** | 必须安装并运行（所有传感器数据来源） |
| **RTSS** | 想看帧数必须有（Afterburner 启动时会自动带起它） |
| Windows | 已测试 Windows 10 / 11 |

程序会自动在常见安装位置搜索这两个程序，找不到可在 `config.json` 手动指定。

### 配置 Afterburner（关键步骤）

1. 打开 MSI Afterburner
2. 点齿轮图标 → **监控** 选项卡
3. 在"活动硬件监控图表"里**选中**你想看的项目
4. 对每个选中项，勾选下方的 **「在 OSD 上显示」**
5. 点「应用」

网页会自动跟随你的勾选。

---

## 使用

### 方式一：直接运行 exe（推荐普通用户）

1. 从 [Releases](../../releases) 下载 `AfterburnerWebMonitor.exe`
2. 双击运行
3. 首次会弹 UAC 提权框（用于读取传感器），点"是"
4. 弹出提示框会显示手机访问地址，例如 `http://192.168.4.9:8777`
5. 手机连同一 WiFi，浏览器打开该地址

### 方式二：从源码运行（开发者）

```bash
pip install pystray Pillow
python AfterburnerWebMonitor.py
```

> Web 服务基于 Python 标准库 `http.server`，**无需 Flask**。

### 自己打包 exe

```bash
pip install pyinstaller
pyinstaller --noconfirm --clean AfterburnerWebMonitor.spec
```

产物在 `dist/AfterburnerWebMonitor.exe`。

---

## 功能说明

### 网页界面

- **帧数卡片**：当前游戏名、实时帧数、1% low / 0.1% low / 帧时间、60 秒帧数折线图
- **传感器卡片**：按 GPU / CPU / 内存 / 其他 自动分组，内容 = 你 OSD 勾选的项目
- **转屏按钮**：一键旋转 90 度，适配手机横屏摆放
- **设置面板**：三项开机自启开关 + 一键启动 Afterburner / RTSS

### 托盘菜单

- 打开网页
- 启动 Afterburner
- 启动 RTSS（帧数）—— 一键以管理员权限启动
- 打开日志
- 退出

### 快捷键 / 操作

| 操作 | 效果 |
|---|---|
| 双击托盘图标 | 打开网页 |
| 右键托盘图标 | 打开菜单 |
| 手机浏览器「添加到主屏幕」 | 像 App 一样全屏使用 |

---

## 配置文件

首次运行自动生成 `config.json`：

```json
{
  "ab_path": "D:\\ComputerTools\\msiafterburner\\MSI Afterburner\\MSIAfterburner.exe",
  "rtss_path": "D:\\ComputerTools\\RivaTuner Statistics Server\\RTSS.exe",
  "port": 8777,
  "auto_start_ab": true,
  "auto_start_rtss": true
}
```

| 字段 | 说明 |
|---|---|
| `ab_path` | MSI Afterburner 的 exe 路径 |
| `rtss_path` | RTSS 的 exe 路径 |
| `port` | 网页端口，冲突可改（默认 8777） |
| `auto_start_ab` | 启动程序时是否自动拉起 Afterburner |
| `auto_start_rtss` | 启动程序时是否自动拉起 RTSS |

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

**Q: 手机打不开？**

- 确认手机和电脑连同一 WiFi
- Windows 防火墙首次会弹窗，选**允许访问**
- 或手动放行：

```bash
netsh advfirewall firewall add rule name="AfterburnerWebMonitor" dir=in action=allow protocol=TCP localport=8777
```

**Q: CPU 温度显示不出来？**

确认 Afterburner 的监控列表里有 CPU 温度项，并且已勾选「在 OSD 上显示」。

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

---

## 限制

- 仅支持 Windows（依赖 Windows 共享内存 API）
- 帧率数据依赖 RTSS，无游戏运行时无帧率
- 只能读取 Afterburner 已支持并配置监控的硬件项

---

## License

MIT

---

## 致谢

- [MSI Afterburner](https://www.msi.com/Landing/afterburner) / [RivaTuner Statistics Server](https://www.guru3d.com/files-details/rtss-rivatuner-statistics-server-download.html) —— 数据源
- Python 标准库 `http.server` —— Web 服务（无需第三方框架）
- [pystray](https://github.com/moses-palmer/pystray) —— 托盘图标
