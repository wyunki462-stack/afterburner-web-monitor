# -*- coding: utf-8 -*-
"""
AfterburnerWebMonitor —— 把 MSI Afterburner 的硬件监控数据变成手机可看的网页

数据源说明：
  1. **MAHM 共享内存**（MSI Afterburner / RivaTuner）—— 传感器数据
     凡是在 Afterburner 的 OSD 上勾选显示的项目，这里就能读到。
     即"你 OSD 上显示什么，网页就显示什么"。
  2. **RTSS 共享内存** —— 帧率数据（帧率、帧时间、1% low 等）

功能：
  1. 自动检测并拉起 MSI Afterburner / RTSS
  2. 无窗口启动网页服务
  3. 弹出提示框显示手机访问地址
  4. 托盘图标，右键可打开网页 / 启动 RTSS / 退出
  5. 网页内可设置开机自启

作者：为个人使用定制
"""
import ctypes
import json
import os
import socket
import sys
import threading
import time
import urllib.request
import webbrowser

# ---------- 常量 ----------
APP_NAME = "AfterburnerWebMonitor"
APP_VERSION = "1.0.0"
PORT = 8777

# 默认程序路径（可在 config.json 里覆盖）
DEFAULT_AB_PATH = r"D:\ComputerTools\msiafterburner\MSI Afterburner\MSIAfterburner.exe"
DEFAULT_RTSS_PATH = r"D:\ComputerTools\RivaTuner Statistics Server\RTSS.exe"
# Afterburner 的常见安装位置（自动探测用）
AB_CANDIDATES = [
    DEFAULT_AB_PATH,
    r"C:\Program Files (x86)\MSI Afterburner\MSIAfterburner.exe",
    r"C:\Program Files\MSI Afterburner\MSIAfterburner.exe",
    r"D:\Program Files (x86)\MSI Afterburner\MSIAfterburner.exe",
]
RTSS_CANDIDATES = [
    DEFAULT_RTSS_PATH,
    r"C:\Program Files (x86)\RivaTuner Statistics Server\RTSS.exe",
    r"C:\Program Files\RivaTuner Statistics Server\RTSS.exe",
]

# ---------- 路径处理（兼容 PyInstaller 打包） ----------
def get_base_dir():
    """程序所在目录（打包后是 exe 所在目录，不是临时解包目录）"""
    if getattr(sys, "frozen", False):
        return os.path.dirname(sys.executable)
    return os.path.dirname(os.path.abspath(__file__))


BASE_DIR = get_base_dir()
CONFIG_FILE = os.path.join(BASE_DIR, "config.json")
LOG_FILE = os.path.join(BASE_DIR, "AfterburnerWebMonitor.log")


# ---------- 日志 ----------
def log(msg):
    try:
        line = f"{time.strftime('%Y-%m-%d %H:%M:%S')} {msg}\n"
        with open(LOG_FILE, "a", encoding="utf-8") as f:
            f.write(line)
    except Exception:
        pass


# ---------- 配置 ----------
DEFAULT_CONFIG = {
    "ab_path": DEFAULT_AB_PATH,
    "rtss_path": DEFAULT_RTSS_PATH,
    "port": PORT,
    "auto_start_ab": True,
    "auto_start_rtss": True,
}


def load_config():
    cfg = dict(DEFAULT_CONFIG)
    if os.path.exists(CONFIG_FILE):
        try:
            with open(CONFIG_FILE, "r", encoding="utf-8") as f:
                cfg.update(json.load(f))
        except Exception as e:
            log(f"读取配置失败: {e}")
    else:
        save_config(cfg)
    return cfg


def save_config(cfg):
    try:
        with open(CONFIG_FILE, "w", encoding="utf-8") as f:
            json.dump(cfg, f, ensure_ascii=False, indent=2)
        return True
    except Exception as e:
        log(f"保存配置失败: {e}")
        return False


def find_exe(cfg_path, candidates):
    """优先用配置里的路径，否则在候选列表里找存在的，最后搜 Afterburner 常见目录"""
    if cfg_path and os.path.exists(cfg_path):
        return cfg_path
    for c in candidates:
        if c and os.path.exists(c):
            return c
    return cfg_path or (candidates[0] if candidates else "")


# ---------- 网络工具 ----------
def get_lan_ip():
    """获取本机局域网 IP（优先 192.168.x / 10.x / 172.x）"""
    candidates = []
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.settimeout(0.5)
        try:
            s.connect(("8.8.8.8", 80))
            candidates.append(s.getsockname()[0])
        finally:
            s.close()
    except Exception:
        pass

    try:
        hostname = socket.gethostname()
        for info in socket.getaddrinfo(hostname, None, socket.AF_INET):
            ip = info[4][0]
            if ip not in candidates:
                candidates.append(ip)
    except Exception:
        pass

    for ip in candidates:
        if ip.startswith("192.168."):
            return ip
    for ip in candidates:
        if ip.startswith(("10.", "172.")):
            return ip
    for ip in candidates:
        if not ip.startswith("127."):
            return ip
    return "127.0.0.1"


def port_open(host, port, timeout=1.0):
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except Exception:
        return False


# ---------- Win32 共享内存 ----------
_k32 = None


def _load_kernel32():
    global _k32
    if _k32 is not None:
        return _k32
    try:
        from ctypes import wintypes
        k = ctypes.WinDLL("kernel32", use_last_error=True)
        k.OpenFileMappingW.restype = wintypes.HANDLE
        k.OpenFileMappingW.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.LPCWSTR]
        k.MapViewOfFile.restype = ctypes.c_void_p
        k.MapViewOfFile.argtypes = [wintypes.HANDLE, wintypes.DWORD,
                                    wintypes.DWORD, wintypes.DWORD, ctypes.c_size_t]
        k.UnmapViewOfFile.argtypes = [ctypes.c_void_p]
        k.CloseHandle.argtypes = [wintypes.HANDLE]
        _k32 = k
    except Exception as e:
        log(f"加载 kernel32 失败: {e}")
        _k32 = False
    return _k32


def _u32(p, off):
    return ctypes.c_uint32.from_address(p + off).value


def _decode_ansi(b):
    """解码 Afterburner 写入的 ANSI 字符串；° 在 GBK 下会变乱码，做一次修正。"""
    s = b.split(b"\x00")[0]
    try:
        txt = s.decode("mbcs", "ignore")
    except Exception:
        txt = s.decode("latin-1", "ignore")
    txt = txt.replace("癈", "°C").replace("癹", "°F").replace("癶", "°")
    return txt.strip()


# ============================================================
#  数据源 1：MSI Afterburner 硬件监控共享内存（MAHM）
#  读取"在 OSD 上显示"的监控项 —— OSD 勾什么，这里就有什么
# ============================================================
MAHM_SHM_NAME = "MAHMSharedMemory"
MAHM_SIG = 0x4D41484D                      # 'MAHM'
MAHM_FLAG_SHOW_IN_OSD = 0x00000001
FLT_MAX = 3.4028234663852886e38            # 该值表示"数据当前不可用"

MAHM_OFF_SIG = 0
MAHM_OFF_HDR_SIZE = 8
MAHM_OFF_NUM_ENTRIES = 12
MAHM_OFF_ENTRY_SIZE = 16

_M = 260                                    # MAX_PATH
MAHM_E_SRCNAME = 0
MAHM_E_UNITS = _M
MAHM_E_DATA = _M * 5                        # float
MAHM_E_FLAGS = MAHM_E_DATA + 12
MAHM_E_GPU = MAHM_E_FLAGS + 4
MAHM_E_SRCID = MAHM_E_GPU + 4

# 源 ID 常量（用于归类）
SRC_GPU_TEMP = 0x00000000
SRC_GPU_MEM_TEMP = 0x00000002
SRC_VRM_TEMP = 0x00000003
SRC_FAN_SPEED = 0x00000010
SRC_FAN_RPM = 0x00000011
SRC_CORE_CLOCK = 0x00000020
SRC_MEM_CLOCK = 0x00000022
SRC_GPU_USAGE = 0x00000030
SRC_MEM_USAGE = 0x00000031
SRC_FB_USAGE = 0x00000032
SRC_VID_USAGE = 0x00000033
SRC_BUS_USAGE = 0x00000034
SRC_FRAMERATE = 0x00000050
SRC_FRAMETIME = 0x00000051
SRC_FPS_MIN = 0x00000052
SRC_FPS_AVG = 0x00000053
SRC_FPS_MAX = 0x00000054
SRC_FPS_1LOW = 0x00000055
SRC_FPS_01LOW = 0x00000056
SRC_GPU_POWER_REL = 0x00000060
SRC_GPU_POWER_ABS = 0x00000061
SRC_CPU_TEMP = 0x00000080
SRC_CPU_USAGE = 0x00000090
SRC_RAM_USAGE = 0x00000091
SRC_PAGEFILE = 0x00000092
SRC_CPU_CLOCK = 0x000000A0
SRC_CPU_POWER = 0x00000100

# 源 ID -> 归类（frontend 用来分组：gpu / cpu / ram / fps / other）
SRC_GROUP = {
    SRC_GPU_TEMP: "gpu", SRC_GPU_MEM_TEMP: "gpu", SRC_VRM_TEMP: "gpu",
    SRC_FAN_SPEED: "gpu", SRC_FAN_RPM: "gpu",
    SRC_CORE_CLOCK: "gpu", SRC_MEM_CLOCK: "gpu",
    SRC_GPU_USAGE: "gpu", SRC_MEM_USAGE: "gpu", SRC_FB_USAGE: "gpu",
    SRC_VID_USAGE: "gpu", SRC_BUS_USAGE: "gpu",
    SRC_GPU_POWER_REL: "gpu", SRC_GPU_POWER_ABS: "gpu",
    SRC_CPU_TEMP: "cpu", SRC_CPU_USAGE: "cpu",
    SRC_CPU_CLOCK: "cpu", SRC_CPU_POWER: "cpu",
    SRC_RAM_USAGE: "ram", SRC_PAGEFILE: "ram",
    SRC_FRAMERATE: "fps", SRC_FRAMETIME: "fps", SRC_FPS_MIN: "fps",
    SRC_FPS_AVG: "fps", SRC_FPS_MAX: "fps",
    SRC_FPS_1LOW: "fps", SRC_FPS_01LOW: "fps",
}

# 源 ID -> 显示短名（Afterburner 的名字如 "GPU2 temperature" 太长，统一美化）
SRC_LABEL = {
    SRC_GPU_TEMP: "温度", SRC_GPU_MEM_TEMP: "显存温度", SRC_VRM_TEMP: "供电温度",
    SRC_FAN_SPEED: "风扇", SRC_FAN_RPM: "风扇转速",
    SRC_CORE_CLOCK: "核心频率", SRC_MEM_CLOCK: "显存频率",
    SRC_GPU_USAGE: "占用率", SRC_MEM_USAGE: "显存占用", SRC_FB_USAGE: "显存控制器",
    SRC_VID_USAGE: "视频引擎", SRC_BUS_USAGE: "总线占用",
    SRC_GPU_POWER_REL: "功耗(TDP%)", SRC_GPU_POWER_ABS: "功耗",
    SRC_CPU_TEMP: "温度", SRC_CPU_USAGE: "占用",
    SRC_CPU_CLOCK: "频率", SRC_CPU_POWER: "功耗",
    SRC_RAM_USAGE: "内存占用", SRC_PAGEFILE: "页面文件",
    SRC_FRAMERATE: "帧率", SRC_FRAMETIME: "帧时间", SRC_FPS_MIN: "最低帧",
    SRC_FPS_AVG: "平均帧", SRC_FPS_MAX: "最高帧",
    SRC_FPS_1LOW: "1% low", SRC_FPS_01LOW: "0.1% low",
}

# 单位归一化（Afterburner 有时用 °C 的乱码形式，或空格）
UNIT_NORM = {
    "癈": "°C", "癹": "°F", "C": "°C", "F": "°F",
    "FPS": "FPS", "fps": "FPS", "ms": "ms", "MHz": "MHz",
    "W": "W", "%": "%", "MB": "MB", "RPM": "RPM",
}

_mahm_handle = None
_mahm_view = None


def _mahm_detach():
    global _mahm_handle, _mahm_view
    k = _load_kernel32()
    if not k:
        return
    try:
        if _mahm_view:
            k.UnmapViewOfFile(ctypes.c_void_p(_mahm_view))
        if _mahm_handle:
            k.CloseHandle(_mahm_handle)
    except Exception:
        pass
    _mahm_handle, _mahm_view = None, None


def read_afterburner():
    """
    读取 Afterburner 的监控数据，只返回"在 OSD 上显示"的项。

    返回:
      {
        "ok": bool,            # Afterburner 是否在运行且共享内存有效
        "items": [             # ★ 已勾选 OSD 的监控项
            {"id": int, "name": str, "label": str, "unit": str,
             "group": str, "gpu": int, "value": float|None}, ...
        ],
        "all_count": int,      # Afterburner 里配置的全部监控项数
        "osd_count": int,      # 其中勾选了 OSD 的数量
      }
    value 为 None 表示该项当前不可用（如没开游戏时的帧率）。
    """
    global _mahm_handle, _mahm_view
    out = {"ok": False, "items": [], "all_count": 0, "osd_count": 0}

    k = _load_kernel32()
    if not k:
        return out

    if _mahm_view is None:
        h = k.OpenFileMappingW(0x0004, False, MAHM_SHM_NAME)   # FILE_MAP_READ
        if not h:
            return out
        p = k.MapViewOfFile(h, 0x0004, 0, 0, 0)
        if not p:
            k.CloseHandle(h)
            return out
        _mahm_handle, _mahm_view = h, p

    p = _mahm_view
    try:
        if _u32(p, MAHM_OFF_SIG) != MAHM_SIG:
            _mahm_detach()
            return out
        hdr_size = _u32(p, MAHM_OFF_HDR_SIZE)
        num = _u32(p, MAHM_OFF_NUM_ENTRIES)
        esize = _u32(p, MAHM_OFF_ENTRY_SIZE)
        if not (0 < num <= 512 and esize > MAHM_E_SRCID):
            return out

        out["ok"] = True
        out["all_count"] = num

        base = p + hdr_size
        seen_keys = set()     # 去重：同 id+同 gpu 只保留第一个
        for i in range(num):
            e = base + i * esize
            if not (_u32(e, MAHM_E_FLAGS) & MAHM_FLAG_SHOW_IN_OSD):
                continue
            src_id = _u32(e, MAHM_E_SRCID)
            gpu = _u32(e, MAHM_E_GPU)
            key = (src_id, gpu)
            if key in seen_keys:
                continue
            seen_keys.add(key)

            raw_name = _decode_ansi(ctypes.string_at(e + MAHM_E_SRCNAME, _M))
            unit = _decode_ansi(ctypes.string_at(e + MAHM_E_UNITS, _M))
            unit = UNIT_NORM.get(unit, unit)
            data = ctypes.c_float.from_address(e + MAHM_E_DATA).value
            val = None
            if data == data and abs(data) < FLT_MAX / 2:   # 排除 NaN / FLT_MAX
                val = round(data, 2)

            group = SRC_GROUP.get(src_id, "other")
            label = SRC_LABEL.get(src_id, raw_name)

            out["items"].append({
                "id": src_id, "name": raw_name, "label": label,
                "unit": unit, "group": group, "gpu": gpu, "value": val,
            })
        out["osd_count"] = len(out["items"])
    except Exception as ex:
        log(f"读 Afterburner 共享内存失败: {ex}")
    return out


def ab_alive():
    return bool(read_afterburner().get("ok"))


# ============================================================
#  数据源 2：RTSS 共享内存 —— 帧率与帧时间统计
# ============================================================
RTSS_SHM_NAME = "RTSSSharedMemoryV2"
RTSS_SIG = 0x52545353          # 'RTSS'

OFF_SIG = 0
OFF_APP_ENTRY_SIZE = 8
OFF_APP_ARR_OFFSET = 12
OFF_APP_ARR_SIZE = 16

ENTRY_OFF_PID = 0
ENTRY_OFF_NAME = 4             # char szName[MAX_PATH=260]
ENTRY_SIZE_NAME = 260
ENTRY_OFF_FLAGS = ENTRY_OFF_NAME + ENTRY_SIZE_NAME            # 264
ENTRY_OFF_TIME0 = ENTRY_OFF_FLAGS + 4                         # 268
ENTRY_OFF_TIME1 = ENTRY_OFF_TIME0 + 4                         # 272
ENTRY_OFF_FRAMES = ENTRY_OFF_TIME1 + 4                        # 276
ENTRY_OFF_FRAMETIME = ENTRY_OFF_FRAMES + 4                    # 280
ENTRY_OFF_STATFLAGS = ENTRY_OFF_FRAMETIME + 4                 # 284
ENTRY_OFF_STATCOUNT = ENTRY_OFF_STATFLAGS + 16                # 300
ENTRY_OFF_STATFPS_MIN = ENTRY_OFF_STATCOUNT + 4               # 304
ENTRY_OFF_STATFPS_AVG = ENTRY_OFF_STATFPS_MIN + 4             # 308
ENTRY_OFF_STATFPS_MAX = ENTRY_OFF_STATFPS_AVG + 4             # 312
# 1% low / 0.1% low（dwStatFrameTimeLowBuf[1024] 之后，已实测校验）
ENTRY_OFF_FPS_1PCT_LOW = 9172
ENTRY_OFF_FPS_01PCT_LOW = 9176

_rtss_handle = None
_rtss_view = None


def _rtss_attach():
    global _rtss_handle, _rtss_view
    if _rtss_view:
        return _rtss_view
    k = _load_kernel32()
    if not k:
        return None
    h = k.OpenFileMappingW(0x0004, False, RTSS_SHM_NAME)
    if not h:
        return None
    p = k.MapViewOfFile(h, 0x0004, 0, 0, 0)
    if not p:
        k.CloseHandle(h)
        return None
    _rtss_handle, _rtss_view = h, p
    return p


def _rtss_detach():
    global _rtss_handle, _rtss_view
    k = _load_kernel32()
    if not k:
        return
    try:
        if _rtss_view:
            k.UnmapViewOfFile(ctypes.c_void_p(_rtss_view))
        if _rtss_handle:
            k.CloseHandle(_rtss_handle)
    except Exception:
        pass
    _rtss_handle, _rtss_view = None, None


def read_rtss_fps():
    """读取 RTSS 当前帧率统计。返回 running / app / fps / 1%low 等。"""
    out = {"running": False, "app": "", "fps": None,
           "fps_min": None, "fps_avg": None, "fps_max": None,
           "fps_1pct_low": None, "fps_01pct_low": None, "frametime": None}
    p = _rtss_attach()
    if p is None:
        return out
    try:
        if _u32(p, OFF_SIG) != RTSS_SIG:     # 0xDEAD ⇒ 未运行
            _rtss_detach()
            return out
        out["running"] = True
        entry_size = _u32(p, OFF_APP_ENTRY_SIZE)
        arr_off = _u32(p, OFF_APP_ARR_OFFSET)
        arr_size = _u32(p, OFF_APP_ARR_SIZE)
        if not (0 < entry_size < 1024 * 1024 and 0 < arr_size <= 256):
            return out

        base = p + arr_off
        best = None
        for i in range(arr_size):
            e = base + i * entry_size
            if _u32(e, ENTRY_OFF_PID) == 0:
                continue
            name = ctypes.string_at(e + ENTRY_OFF_NAME, ENTRY_SIZE_NAME)\
                    .split(b"\x00")[0].decode("mbcs", "ignore")
            t0 = _u32(e, ENTRY_OFF_TIME0)
            t1 = _u32(e, ENTRY_OFF_TIME1)
            frames = _u32(e, ENTRY_OFF_FRAMES)
            frametime = _u32(e, ENTRY_OFF_FRAMETIME)
            fps = None
            if t1 > t0 and frames > 0:
                fps = 1000.0 * frames / (t1 - t0)
            elif 0 < frametime < 10_000_000:
                fps = 1000000.0 / frametime
            if not fps or fps <= 0:
                continue
            if best is None or fps > best[0]:
                cnt = _u32(e, ENTRY_OFF_STATCOUNT)
                best = (fps, name, frametime,
                        _u32(e, ENTRY_OFF_STATFPS_MIN) if cnt else None,
                        _u32(e, ENTRY_OFF_STATFPS_AVG) if cnt else None,
                        _u32(e, ENTRY_OFF_STATFPS_MAX) if cnt else None,
                        _u32(e, ENTRY_OFF_FPS_1PCT_LOW) if cnt else None,
                        _u32(e, ENTRY_OFF_FPS_01PCT_LOW) if cnt else None)
        if best:
            fps, name, frametime, mn, avg, mx, low1, low01 = best
            out.update({
                "fps": round(fps, 1), "app": name,
                "frametime": round(frametime / 1000.0, 2) if frametime else None,
                "fps_min": mn, "fps_avg": avg, "fps_max": mx,
                "fps_1pct_low": low1, "fps_01pct_low": low01,
            })
    except Exception as e:
        log(f"读 RTSS 失败: {e}")
    return out


def rtss_alive():
    return bool(read_rtss_fps().get("running"))


# ---------- 60 秒帧率历史 ----------
FPS_HISTORY_LEN = 60
_fps_history = []
_fps_history_lock = threading.Lock()


def _push_fps_history(rtss):
    now = time.time()
    fps = rtss.get("fps") if rtss and rtss.get("running") else None
    with _fps_history_lock:
        _fps_history.append((now, fps))
        if len(_fps_history) > FPS_HISTORY_LEN:
            del _fps_history[:len(_fps_history) - FPS_HISTORY_LEN]


def get_fps_history():
    with _fps_history_lock:
        return list(_fps_history)


# ---------- 进程管理 ----------
def start_program(path, elevate=False):
    """启动程序。elevate=True 时用 runas 提权（弹 UAC）。"""
    if not path or not os.path.exists(path):
        log(f"路径不存在: {path}")
        return False
    try:
        if elevate:
            SW_SHOWNORMAL = 1
            r = ctypes.windll.shell32.ShellExecuteW(
                None, "runas", path, None, os.path.dirname(path), SW_SHOWNORMAL)
            if int(r) > 32:
                log(f"已提权启动: {path}")
                return True
            log(f"提权启动失败(返回 {r})，改用普通方式")
        os.startfile(path)
        log(f"已启动: {path}")
        return True
    except Exception as e:
        log(f"启动失败 {path}: {e}")
        return False


def wait_ab_ready(timeout=25):
    t0 = time.time()
    while time.time() - t0 < timeout:
        if ab_alive():
            return True
        time.sleep(1)
    return False


def wait_rtss_ready(timeout=15):
    t0 = time.time()
    while time.time() - t0 < timeout:
        if rtss_alive():
            return True
        time.sleep(1)
    return False


# ---------- 开机自启（注册表 Run 键，仅当前用户） ----------
RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"
AUTOSTART_ENTRIES = {
    "app": APP_NAME,              # AfterburnerWebMonitor 本体
    "ab": "AfterburnerWebMonitor_AB",         # MSI Afterburner
    "rtss": "AfterburnerWebMonitor_RTSS",     # RTSS
}


def _winreg():
    try:
        import winreg
        return winreg
    except ImportError:
        return None


def autostart_get(which):
    winreg = _winreg()
    if not winreg:
        return False
    name = AUTOSTART_ENTRIES.get(which)
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as k:
            try:
                winreg.QueryValueEx(k, name)
                return True
            except (FileNotFoundError, OSError):
                return False
    except Exception:
        return False


def autostart_set(which, enable, target_path=None):
    """设置/取消开机自启，返回 (成功, 说明)。"""
    winreg = _winreg()
    if not winreg:
        return False, "系统不支持 winreg"
    name = AUTOSTART_ENTRIES.get(which)
    if not name:
        return False, f"未知的自启项: {which}"

    cmd = ""
    if enable:
        if which == "app":
            if getattr(sys, "frozen", False):
                cmd = f'"{sys.executable}"'
            else:
                pyw = os.path.join(os.path.dirname(sys.executable), "pythonw.exe")
                if not os.path.exists(pyw):
                    pyw = sys.executable
                cmd = f'"{pyw}" "{os.path.abspath(__file__)}"'
        else:
            if not target_path or not os.path.exists(target_path):
                return False, f"路径不存在: {target_path}"
            cmd = f'"{target_path}"'
    # enable=False 时不校验路径：程序可能已删除，但注册表残留仍需清除

    try:
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as k:
            if enable:
                winreg.SetValueEx(k, name, 0, winreg.REG_SZ, cmd)
                log(f"已设置开机自启: {name} -> {cmd}")
                return True, "已开启"
            try:
                winreg.DeleteValue(k, name)
                log(f"已取消开机自启: {name}")
            except (FileNotFoundError, OSError):
                pass
            return True, "已关闭"
    except Exception as e:
        log(f"设置开机自启失败 {name}: {e}")
        return False, str(e)


def autostart_status():
    return {k: autostart_get(k) for k in AUTOSTART_ENTRIES}


# ---------- 采集循环 ----------
_cache = {"ts": 0, "ab": {}, "rtss": {}, "ab_ok": False}
_cache_lock = threading.Lock()


def collector_loop():
    while True:
        # 传感器数据（Afterburner OSD 项）
        try:
            ab = read_afterburner()
        except Exception:
            ab = {"ok": False, "items": [], "all_count": 0, "osd_count": 0}
        # 帧率数据（RTSS）
        try:
            rtss = read_rtss_fps()
        except Exception:
            rtss = {"running": False}

        with _cache_lock:
            _cache["ab"] = ab
            _cache["ab_ok"] = bool(ab.get("ok"))
            _cache["rtss"] = rtss
            _cache["ts"] = time.time()
        _push_fps_history(rtss)
        time.sleep(1)


# ---------- Web 服务 ----------
from flask import Flask, jsonify, render_template_string  # noqa: E402
import logging  # noqa: E402

app = Flask(APP_NAME)
logging.getLogger("werkzeug").setLevel(logging.ERROR)
app.logger.setLevel(logging.ERROR)


@app.route("/api/data")
def api_data():
    with _cache_lock:
        return jsonify(dict(_cache))


@app.route("/api/fps_history")
def api_fps_history():
    hist = get_fps_history()
    now = time.time()
    return jsonify({
        "points": [{"t": round(now - t, 1), "fps": f} for t, f in hist],
        "len": FPS_HISTORY_LEN,
    })


@app.route("/api/settings", methods=["GET", "POST"])
def api_settings():
    from flask import request
    cfg = load_config()

    if request.method == "GET":
        ab_path = find_exe(cfg.get("ab_path"), AB_CANDIDATES)
        rtss_path = find_exe(cfg.get("rtss_path"), RTSS_CANDIDATES)
        return jsonify({
            "autostart": autostart_status(),
            "config": {
                "port": cfg.get("port", PORT),
                "ab_path": ab_path,
                "rtss_path": rtss_path,
                "auto_start_ab": cfg.get("auto_start_ab", True),
                "auto_start_rtss": cfg.get("auto_start_rtss", True),
            },
            "paths_exist": {
                "ab": os.path.exists(ab_path),
                "rtss": os.path.exists(rtss_path),
            },
        })

    body = request.get_json(silent=True) or {}
    results = {}
    if "autostart" in body:
        a = body["autostart"] or {}
        if "app" in a:
            results["app"] = autostart_set("app", bool(a["app"]))
        if "ab" in a:
            results["ab"] = autostart_set(
                "ab", bool(a["ab"]),
                find_exe(cfg.get("ab_path"), AB_CANDIDATES))
        if "rtss" in a:
            results["rtss"] = autostart_set(
                "rtss", bool(a["rtss"]),
                find_exe(cfg.get("rtss_path"), RTSS_CANDIDATES))

    changed = False
    for key in ("auto_start_ab", "auto_start_rtss", "port"):
        if key in body:
            cfg[key] = body[key]
            changed = True
    if changed:
        save_config(cfg)

    return jsonify({"ok": True, "results": results,
                    "autostart": autostart_status()})


@app.route("/api/start/<what>", methods=["POST"])
def api_start(what):
    """从网页一键启动 Afterburner / RTSS。"""
    cfg = load_config()
    if what == "ab":
        if ab_alive():
            return jsonify({"ok": True, "msg": "Afterburner 已在运行"})
        path = find_exe(cfg.get("ab_path"), AB_CANDIDATES)
        ok = start_program(path, elevate=False)   # AB 自己有提权机制
        return jsonify({"ok": ok, "msg": "已请求启动 Afterburner" if ok
                        else "启动失败，请检查 Afterburner 路径"})
    if what == "rtss":
        if rtss_alive():
            return jsonify({"ok": True, "msg": "RTSS 已在运行"})
        path = find_exe(cfg.get("rtss_path"), RTSS_CANDIDATES)
        ok = start_program(path, elevate=True)
        return jsonify({"ok": ok, "msg": "已请求启动 RTSS（请在弹出的窗口点\"是\"）"
                        if ok else "启动失败，请检查 RTSS 路径"})
    return jsonify({"ok": False, "msg": "未知的目标"}), 400


PAGE = r"""<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no">
<meta name="apple-mobile-web-app-capable" content="yes">
<meta name="theme-color" content="#0f1117">
<title>硬件监控</title>
<style>
*{margin:0;padding:0;box-sizing:border-box;-webkit-tap-highlight-color:transparent}
body{background:#0f1117;color:#e8eaf0;font-family:-apple-system,"PingFang SC","Microsoft YaHei",sans-serif;
     padding:14px;min-height:100vh}
body.rotate{transform:rotate(90deg);transform-origin:left top;width:100vh;height:100vw;
     position:absolute;top:0;left:100vw;overflow-y:auto}
.header{display:flex;justify-content:space-between;align-items:center;margin-bottom:14px}
.header h1{font-size:17px;font-weight:600;letter-spacing:.5px}
.status{font-size:11px;color:#5b6272;display:flex;align-items:center;gap:5px}
.dot{width:7px;height:7px;border-radius:50%;background:#2ecc71;animation:pulse 1.4s infinite}
.dot.off{background:#e74c3c;animation:none}
@keyframes pulse{0%,100%{opacity:1}50%{opacity:.25}}
.grid{display:grid;grid-template-columns:1fr 1fr;gap:10px;margin-bottom:12px}
.grid3{display:grid;grid-template-columns:1fr 1fr 1fr;gap:8px;margin-bottom:12px}
.card{background:#1a1d27;border-radius:14px;padding:14px 12px;position:relative;overflow:hidden;
      border:1px solid #232733}
.grid3 .card{padding:11px 9px}
.grid3 .value{font-size:20px}
.label{font-size:11px;color:#7a8296;margin-bottom:7px;
      white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.value{font-size:25px;font-weight:700;font-variant-numeric:tabular-nums;line-height:1.1}
.value .unit{font-size:12px;font-weight:500;color:#7a8296;margin-left:2px}
.bar{height:5px;background:#252a37;border-radius:3px;margin-top:9px;overflow:hidden}
.bar > i{display:block;height:100%;border-radius:3px;transition:width .5s ease,background .5s ease}
.section-tag{font-size:12px;color:#6b7385;font-weight:600;margin:16px 0 8px;
             letter-spacing:1.5px;text-transform:uppercase}
.t-cool{color:#3fa9f5}.t-warm{color:#f5c542}.t-hot{color:#ff5c5c}
.footer{text-align:center;font-size:10px;color:#4a5164;margin-top:16px;padding-bottom:8px}
.rotbtn{background:#232733;border:none;color:#9aa3b8;font-size:11px;padding:5px 10px;
        border-radius:8px;cursor:pointer}
.rotbtn:active{background:#2d3342}
.warn{background:#2a1f1f;border:1px solid #4a2626;color:#ff8a8a;font-size:11px;
      padding:10px 12px;border-radius:8px;margin-bottom:10px;line-height:1.6}
.warn b{color:#ffb3b3}

/* 帧数卡片 */
.fps-card{background:#1a1d27;border:1px solid #232733;border-radius:14px;
      padding:14px 12px;margin-bottom:12px;position:relative;overflow:hidden}
.fps-card .app-name{font-size:12px;color:#7a8296;margin-bottom:7px;
      white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:80%}
.fps-card .row{display:flex;align-items:flex-end;justify-content:space-between;gap:10px}
.fps-card .fps-val{font-size:32px;font-weight:700;line-height:1.05;
      font-variant-numeric:tabular-nums;color:#7c5cff}
.fps-card .fps-val.idle{color:#4a5164}
.fps-card .fps-val .unit{font-size:12px;font-weight:500;color:#7a8296;margin-left:3px}
.fps-card .lows{display:flex;gap:14px;font-size:11px;color:#7a8296;
      font-variant-numeric:tabular-nums;padding-bottom:3px;flex-wrap:wrap;justify-content:flex-end}
.fps-card .lows b{color:#e8eaf0;font-weight:600}
.spark{margin-top:10px;width:100%;height:56px;display:block}
.spark-empty{height:56px;display:flex;align-items:center;justify-content:center;
      font-size:11px;color:#4a5164;margin-top:10px;
      border-top:1px dashed #232733}

/* 设置面板 */
.mask{position:fixed;inset:0;background:rgba(0,0,0,.62);display:none;z-index:50;
      align-items:flex-end;justify-content:center}
.mask.on{display:flex}
.panel{background:#161a24;width:100%;max-width:520px;border-radius:18px 18px 0 0;
      padding:18px 16px 26px;max-height:88vh;overflow-y:auto;
      border-top:1px solid #2a3040}
.panel h2{font-size:15px;font-weight:600;margin-bottom:4px}
.panel .sub{font-size:11px;color:#6b7385;margin-bottom:16px}
.set-item{display:flex;justify-content:space-between;align-items:center;
      padding:13px 0;border-bottom:1px solid #1f2432}
.set-item:last-of-type{border-bottom:none}
.set-item .t{font-size:13px;color:#dfe3ec}
.set-item .d{font-size:11px;color:#6b7385;margin-top:3px;line-height:1.5}
.set-item .d.warn-t{color:#e8a33d}
.sw{position:relative;width:44px;height:25px;flex:none;margin-left:12px}
.sw input{opacity:0;width:0;height:0;position:absolute}
.sw i{position:absolute;inset:0;background:#2b3040;border-radius:13px;
      transition:.22s;cursor:pointer}
.sw i:before{content:"";position:absolute;width:19px;height:19px;left:3px;top:3px;
      background:#8a90a0;border-radius:50%;transition:.22s}
.sw input:checked + i{background:#5a3fd6}
.sw input:checked + i:before{transform:translateX(19px);background:#fff}
.sw input:disabled + i{opacity:.45;cursor:not-allowed}
.closebar{margin-top:18px;display:flex;gap:10px}
.btn{flex:1;background:#232733;border:none;color:#cfd4e0;font-size:13px;
      padding:11px;border-radius:11px;cursor:pointer}
.btn.pri{background:#5a3fd6;color:#fff}
.btn:active{opacity:.85}
.toast{position:fixed;left:50%;bottom:34px;transform:translateX(-50%) translateY(20px);
      background:#2d3342;color:#e8eaf0;font-size:12px;padding:10px 18px;border-radius:10px;
      opacity:0;pointer-events:none;transition:.25s;z-index:99;
      border:1px solid #3a4256;max-width:88vw;text-align:center}
.toast.on{opacity:1;transform:translateX(-50%) translateY(0)}
.hint{font-size:11px;color:#5b6272;line-height:1.6;margin-top:10px;
      padding:10px 12px;background:#141822;border-radius:9px;border:1px solid #1f2432}
</style>
</head>
<body>
<div class="header">
  <h1>硬件监控</h1>
  <div style="display:flex;gap:8px;align-items:center">
    <button class="rotbtn" onclick="openSet()">设置</button>
    <button class="rotbtn" onclick="toggleRotate()">转屏</button>
    <div class="status"><span class="dot" id="dot"></span><span id="stat">连接中</span></div>
  </div>
</div>

<div id="warnWrap"></div>
<div id="fpsWrap"></div>
<div id="gpuWrap"></div>
<div id="cpuWrap"></div>
<div id="ramWrap"></div>
<div id="otherWrap"></div>

<div class="footer" id="foot">—</div>

<div class="mask" id="mask" onclick="if(event.target===this)closeSet()">
  <div class="panel">
    <h2>设置</h2>
    <div class="sub">数据来自 MSI Afterburner —— 在它的 OSD 上勾选什么，这里就显示什么</div>

    <div class="set-item">
      <div>
        <div class="t">AfterburnerWebMonitor 开机自启</div>
        <div class="d">开机后自动启动本监控服务</div>
      </div>
      <label class="sw"><input type="checkbox" id="sw_app" onchange="saveSet()"><i></i></label>
    </div>

    <div class="set-item">
      <div>
        <div class="t">MSI Afterburner 开机自启</div>
        <div class="d warn-t">不启动 Afterburner 就没有任何数据</div>
        <div class="d">传感器数据（温度 / 占用 / 频率）都来自它</div>
      </div>
      <label class="sw"><input type="checkbox" id="sw_ab" onchange="saveSet()"><i></i></label>
    </div>

    <div class="set-item">
      <div>
        <div class="t">RTSS 开机自启</div>
        <div class="d warn-t">不启动 RTSS 就无法读取帧数</div>
        <div class="d">RTSS 负责统计游戏帧率（Afterburner 会带起它）</div>
      </div>
      <label class="sw"><input type="checkbox" id="sw_rtss" onchange="saveSet()"><i></i></label>
    </div>

    <div class="set-item">
      <div>
        <div class="t">启动 Afterburner</div>
        <div class="d" id="abState">检测中…</div>
      </div>
      <button class="rotbtn" onclick="startProg('ab')">启动</button>
    </div>

    <div class="set-item">
      <div>
        <div class="t">启动 RTSS</div>
        <div class="d" id="rtssState">检测中…</div>
      </div>
      <button class="rotbtn" onclick="startProg('rtss')">启动</button>
    </div>

    <div class="hint" id="osdHint">—</div>

    <div class="closebar">
      <button class="btn pri" onclick="closeSet()">完成</button>
    </div>
  </div>
</div>

<div class="toast" id="toast"></div>

<script>
function barColor(pct){ return pct<60?'#2ecc71':(pct<85?'#f5c542':'#e74c3c'); }
function bar(pct){
  if(pct==null) return '';
  return `<div class="bar"><i style="width:${Math.min(pct,100)}%;background:${barColor(pct)}"></i></div>`;
}
function tempCls(t){ return t==null?'':(t<60?'t-cool':(t<80?'t-warm':'t-hot')); }
function num(v,d){ return v==null?'--':Number(v).toFixed(d==null?0:d); }
function esc(s){ return String(s==null?'':s).replace(/[&<>"]/g, c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c])); }

/* ---------- 折线图（60 秒帧率） ---------- */
let hist = [];
function drawSpark(){
  const W = 300, H = 56, PAD = 3;
  const pts = hist;
  if (pts.length < 2) return '<div class="spark-empty">等待帧率数据…</div>';
  const vals = pts.map(p=>p.fps).filter(v=>v!=null);
  if (!vals.length) return '<div class="spark-empty">RTSS 就绪，等待游戏运行…</div>';
  let mx = Math.max.apply(null, vals), mn = Math.min.apply(null, vals);
  if (mx === mn) { mx += 1; mn -= 1; }
  const lo = Math.max(0, mn - (mx-mn)*0.15), hi = mx + (mx-mn)*0.12;
  const n = pts.length;
  const X = i => PAD + i * (W - PAD*2) / Math.max(n-1, 1);
  const Y = v => H - PAD - (v - lo) / (hi - lo) * (H - PAD*2);
  const segs = []; let seg = [];
  pts.forEach((p,i)=>{
    if (p.fps == null) { if (seg.length) { segs.push(seg); seg = []; } return; }
    seg.push([X(i), Y(p.fps)]);
  });
  if (seg.length) segs.push(seg);
  let paths = '';
  segs.forEach(s=>{
    if (s.length === 1) {
      paths += `<circle cx="${s[0][0].toFixed(1)}" cy="${s[0][1].toFixed(1)}" r="1.6" fill="#7c5cff"/>`;
      return;
    }
    const d = s.map((q,i)=> (i?'L':'M') + q[0].toFixed(1) + ' ' + q[1].toFixed(1)).join(' ');
    paths += `<path d="${d} L ${s[s.length-1][0].toFixed(1)} ${H-PAD} L ${s[0][0].toFixed(1)} ${H-PAD} Z" fill="rgba(124,92,255,.13)" stroke="none"/>`;
    paths += `<path d="${d}" fill="none" stroke="#7c5cff" stroke-width="1.8" stroke-linejoin="round" stroke-linecap="round"/>`;
  });
  return `<svg class="spark" viewBox="0 0 ${W} ${H}" preserveAspectRatio="none">
    <line x1="0" y1="${H-PAD}" x2="${W}" y2="${H-PAD}" stroke="#232733" stroke-width="1"/>
    ${paths}
    <text x="${W-2}" y="11" text-anchor="end" font-size="9" fill="#5b6272">${Math.round(hi)}</text>
    <text x="${W-2}" y="${H-4}" text-anchor="end" font-size="9" fill="#5b6272">${Math.round(lo)}</text>
  </svg>`;
}

/* ---------- 帧数卡片（数据来自 RTSS） ---------- */
function renderFps(r){
  if (!r || !r.running) return '';
  const fps = r.fps != null ? r.fps : null;
  const app = r.app || 'RTSS 已就绪';
  let main = fps != null
    ? `<div class="fps-val">${num(fps,1)}<span class="unit">FPS</span></div>`
    : `<div class="fps-val idle">--<span class="unit">FPS</span></div>`;
  let lows = '';
  if (fps != null) {
    const parts = [];
    if (r.fps_1pct_low != null)  parts.push(`<span>1% low <b>${num(r.fps_1pct_low)}</b></span>`);
    if (r.fps_01pct_low != null) parts.push(`<span>0.1% low <b>${num(r.fps_01pct_low)}</b></span>`);
    if (r.frametime != null)     parts.push(`<span>帧时间 <b>${r.frametime}</b> ms</span>`);
    if (parts.length) lows = `<div class="lows">${parts.join('')}</div>`;
  }
  return `<div class="fps-card">
    <div class="app-name">${esc(app)}</div>
    <div class="row">${main}${lows}</div>
    ${drawSpark()}
  </div>`;
}

/* ---------- 通用卡片（数据来自 Afterburner OSD 项） ---------- */
function card(it){
  const val = it.value;
  const unit = it.unit || '';
  let cls = '';
  // 温度类着色
  if ((it.id===0x00 || it.id===0x80) && val!=null) cls = tempCls(val);
  // 百分比类显示进度条
  let b = '';
  if (unit === '%' && val != null) {
    const pct = (it.id===0x60) ? val : val;   // 相对功耗也是 %
    b = bar(pct);
  }
  const showUnit = unit && unit !== '';
  return `<div class="card">
    <div class="label">${esc(it.label)}</div>
    <div class="value ${cls}">${num(val, val!=null && Math.abs(val)<1000 && val%1!==0 ? 1 : 0)}${showUnit?`<span class="unit">${esc(unit)}</span>`:''}</div>
    ${b}
  </div>`;
}

function renderGroup(items, title, cols){
  if (!items.length) return '';
  const cls = cols===3 ? 'grid3' : 'grid';
  return `<div class="section-tag">${title}</div><div class="${cls}">`
       + items.map(card).join('') + `</div>`;
}

function render(d){
  const ab = d.ab || {};
  const items = ab.items || [];
  const abOk = !!ab.ok;
  const rt = d.rtss || {};

  /* ---- 警告条 ---- */
  let warn = '';
  if (!abOk) {
    warn = `<div class="warn">⚠ <b>Afterburner 未运行</b> —— 没有传感器数据。<br>
      请在 <b>设置</b> 里启动它，或手动打开 MSI Afterburner。</div>`;
  }
  document.getElementById('warnWrap').innerHTML = warn;

  /* ---- 帧数卡片 ---- */
  document.getElementById('fpsWrap').innerHTML = renderFps(rt);

  /* ---- 按分组渲染 OSD 项 ---- */
  const byGroup = {gpu:[], cpu:[], ram:[], fps:[], other:[]};
  items.forEach(it => (byGroup[it.group] || byGroup.other).push(it));

  /* 帧率相关的项不重复显示（已在帧数卡片里），除非 RTSS 没运行 */
  if (rt.running) byGroup.fps = [];

  document.getElementById('gpuWrap').innerHTML  = renderGroup(byGroup.gpu, 'GPU');
  document.getElementById('cpuWrap').innerHTML  = renderGroup(byGroup.cpu, 'CPU');
  document.getElementById('ramWrap').innerHTML  = renderGroup(byGroup.ram, '内存');
  document.getElementById('otherWrap').innerHTML= renderGroup(byGroup.other, '其他');

  /* ---- 没有 OSD 项时给个提示 ---- */
  if (abOk && items.length === 0) {
    document.getElementById('gpuWrap').innerHTML =
      `<div class="warn">Afterburner 在运行，但没有勾选任何 OSD 监控项。<br>
       请打开 Afterburner 的 <b>设置 → 监控</b>，<br>
       对想看的项目逐个点 <b>「在 OSD 上显示」</b>。</div>`;
  }

  /* ---- 状态灯 ---- */
  let ok = abOk, txt = abOk ? '实时' : '无数据';
  if (rt.running && rt.fps != null) { ok = true; txt = '实时 · '+num(rt.fps)+'fps'; }
  document.getElementById('dot').className = ok ? 'dot' : 'dot off';
  document.getElementById('stat').textContent = txt;

  const t = d.ts ? new Date(d.ts*1000) : null;
  const cnt = items.length ? ` · OSD ${items.length} 项` : '';
  document.getElementById('foot').textContent =
    (t ? '更新 ' + t.toLocaleTimeString('zh-CN') : '—') + cnt;
}

async function tick(){
  try{
    const r = await fetch('/api/data',{cache:'no-store'});
    const d = await r.json();
    try{
      const h = await fetch('/api/fps_history',{cache:'no-store'});
      hist = (await h.json()).points || [];
    }catch(e){}
    render(d);
  }catch(e){
    document.getElementById('dot').className='dot off';
    document.getElementById('stat').textContent='断开';
  }
}
function toggleRotate(){ document.body.classList.toggle('rotate'); }

/* ---------- 设置面板 ---------- */
let settingsLoaded = false;
function toast(msg){
  const t = document.getElementById('toast');
  t.textContent = msg; t.classList.add('on');
  clearTimeout(t._tm);
  t._tm = setTimeout(()=>t.classList.remove('on'), 2400);
}
function openSet(){ document.getElementById('mask').classList.add('on'); loadSet(); }
function closeSet(){ document.getElementById('mask').classList.remove('on'); }

async function loadSet(){
  try{
    const r = await fetch('/api/settings',{cache:'no-store'});
    const d = await r.json();
    const a = d.autostart || {};
    document.getElementById('sw_app').checked  = !!a.app;
    document.getElementById('sw_ab').checked   = !!a.ab;
    document.getElementById('sw_rtss').checked = !!a.rtss;
    const pe = d.paths_exist || {};
    document.getElementById('sw_ab').disabled   = !pe.ab;
    document.getElementById('sw_rtss').disabled = !pe.rtss;
    settingsLoaded = true;
    refreshStates();
  }catch(e){ toast('读取设置失败'); }
}

async function refreshStates(){
  try{
    const r = await fetch('/api/data',{cache:'no-store'});
    const d = await r.json();
    const ab = d.ab || {}, rt = d.rtss || {};
    document.getElementById('abState').textContent =
      ab.ok ? `运行中 · 共 ${ab.all_count||0} 项，OSD ${ab.osd_count||0} 项`
            : '未运行 —— 没有任何数据';
    document.getElementById('rtssState').textContent =
      rt.running ? (rt.fps!=null ? `运行中 · ${num(rt.fps)} fps` : '运行中 · 等待游戏')
                 : '未运行 —— 没有帧数数据';
    document.getElementById('osdHint').innerHTML = ab.ok
      ? `当前 OSD 显示 <b style="color:#9aa3b8">${ab.osd_count||0}</b> 项，共配置 ${ab.all_count||0} 项。<br>
         想改显示内容？打开 Afterburner → 设置 → 监控，勾选需要项。<br>
         网页会自动跟随，<b style="color:#9aa3b8">不用改任何代码</b>。`
      : `Afterburner 未运行。启动后这里会显示它配置的监控项数量。`;
  }catch(e){}
}

async function saveSet(){
  if(!settingsLoaded) return;
  const body = { autostart: {
    app:  document.getElementById('sw_app').checked,
    ab:   document.getElementById('sw_ab').checked,
    rtss: document.getElementById('sw_rtss').checked,
  }};
  try{
    await fetch('/api/settings',{
      method:'POST', headers:{'Content-Type':'application/json'},
      body: JSON.stringify(body)
    });
    toast('设置已保存');
  }catch(e){ toast('保存失败'); }
}

async function startProg(what){
  const el = document.getElementById(what==='ab' ? 'abState' : 'rtssState');
  el.textContent = '正在启动…';
  try{
    const r = await fetch('/api/start/'+what,{method:'POST'});
    const d = await r.json();
    toast(d.msg || '已请求启动');
    setTimeout(refreshStates, 3000);
  }catch(e){ toast('启动失败'); }
}

tick();
setInterval(tick, 1000);
setInterval(()=>{ if(document.getElementById('mask').classList.contains('on')) refreshStates(); }, 3000);
</script>
</body>
</html>"""


@app.route("/")
def index():
    return render_template_string(PAGE)


def run_web(port):
    app.run(host="0.0.0.0", port=port, threaded=True, debug=False, use_reloader=False)


# ---------- 托盘图标 ----------
def create_tray_image():
    try:
        from PIL import Image, ImageDraw
    except ImportError:
        return None
    size = 64
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([2, 2, size - 2, size - 2], radius=14, fill=(124, 92, 255, 255))
    d.rectangle([18, 16, 24, 48], fill=(255, 255, 255, 255))
    d.rectangle([18, 42, 44, 48], fill=(255, 255, 255, 255))
    return img


def run_tray(port, url):
    try:
        import pystray
        from pystray import MenuItem as Item
    except ImportError:
        log("pystray 未安装，跳过托盘")
        return False

    img = create_tray_image()
    if img is None:
        log("Pillow 未安装，跳过托盘")
        return False

    cfg = load_config()

    def on_open(icon, item):
        webbrowser.open(url)

    def on_start_ab(icon, item):
        if not ab_alive():
            start_program(find_exe(cfg.get("ab_path"), AB_CANDIDATES))

    def on_start_rtss(icon, item):
        if not rtss_alive():
            start_program(find_exe(cfg.get("rtss_path"), RTSS_CANDIDATES),
                          elevate=True)

    def on_quit(icon, item):
        log("用户从托盘退出")
        icon.stop()
        os._exit(0)

    menu = pystray.Menu(
        Item(f"打开网页  {url}", on_open, default=True),
        Item("启动 Afterburner", on_start_ab),
        Item("启动 RTSS（帧数）", on_start_rtss),
        Item("打开日志", lambda i, it: os.startfile(LOG_FILE)
             if os.path.exists(LOG_FILE) else None),
        pystray.Menu.SEPARATOR,
        Item("退出 " + APP_NAME, on_quit),
    )
    icon = pystray.Icon(APP_NAME, img, f"{APP_NAME} - 运行中", menu)
    threading.Thread(target=icon.run, daemon=True).start()
    return True


# ---------- 主流程 ----------
def msgbox(text, title=APP_NAME, error=False):
    try:
        flags = 0x10 if error else 0x40
        ctypes.windll.user32.MessageBoxW(0, text, title, flags)
    except Exception:
        print(text)


def main():
    log(f"=== {APP_NAME} v{APP_VERSION} 启动 ===")
    cfg = load_config()
    port = cfg.get("port", PORT)

    # 1. 端口占用检查
    if port_open("127.0.0.1", port):
        msgbox(f"端口 {port} 已被占用。\n\n可能服务已在运行。\n"
               f"请先在任务管理器结束 python.exe / AfterburnerWebMonitor.exe 后重试。",
               APP_NAME, error=True)
        log(f"端口 {port} 被占用，退出")
        return

    ab_path = find_exe(cfg.get("ab_path"), AB_CANDIDATES)
    rtss_path = find_exe(cfg.get("rtss_path"), RTSS_CANDIDATES)
    log(f"Afterburner: {ab_path}")
    log(f"RTSS: {rtss_path}")

    # 2. 按需启动 Afterburner
    ab_ok = ab_alive()
    if not ab_ok and cfg.get("auto_start_ab", True):
        log("Afterburner 未运行，尝试启动")
        if start_program(ab_path):
            ab_ok = wait_ab_ready(timeout=30)
    log(f"Afterburner 状态: {'就绪' if ab_ok else '未运行'}")

    # 3. 按需启动 RTSS
    rtss_ok = rtss_alive()
    if not rtss_ok and cfg.get("auto_start_rtss", True):
        log("RTSS 未运行，尝试启动")
        start_program(rtss_path, elevate=True)
        rtss_ok = wait_rtss_ready(timeout=15)
    log(f"RTSS 状态: {'就绪' if rtss_ok else '未运行'}")

    # 4. 采集线程
    threading.Thread(target=collector_loop, daemon=True).start()

    # 5. Web 服务
    threading.Thread(target=run_web, args=(port,), daemon=True).start()
    time.sleep(1.5)

    # 6. 局域网地址
    ip = get_lan_ip()
    url = f"http://{ip}:{port}"
    log(f"手机访问地址: {url}")

    # 7. 托盘
    tray_ok = run_tray(port, url)

    # 8. 提示框
    lines = []
    if not ab_ok:
        lines.append("⚠ 未检测到 MSI Afterburner\n   没有传感器数据，请手动启动它。")
    if not rtss_ok:
        lines.append("⚠ 未检测到 RTSS\n   没有帧数显示，可从托盘菜单启动。")
    extra = ("\n\n" + "\n".join(lines)) if lines else ""

    tip = (f"{APP_NAME} 已启动\n"
           f"\n手机浏览器访问：\n{url}"
           f"{extra}\n\n"
           f"数据来自 Afterburner 的 OSD 勾选项，\n"
           f"在 Afterburner 里改勾选，网页自动跟随。\n\n"
           f"程序在后台运行，可从托盘图标退出。")
    if not tray_ok:
        tip += "\n\n(托盘图标不可用，可在任务管理器结束本程序)"

    log("弹窗提示")
    msgbox(tip)

    # 9. 保活
    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        log("收到中断，退出")
    finally:
        log("=== 程序退出 ===")


if __name__ == "__main__":
    main()
