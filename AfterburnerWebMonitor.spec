# -*- mode: python ; coding: utf-8 -*-


a = Analysis(
    ['AfterburnerWebMonitor.py'],
    pathex=[],
    binaries=[],
    datas=[],
    hiddenimports=[],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        # Web 层已改用标准库 http.server，彻底排除 Flask 全家桶
        'flask', 'werkzeug', 'jinja2', 'markupsafe', 'itsdangerous',
        'click', 'blinker',
        # 只排除确定用不到的大型库（注意：email/html/http 是 http.server
        # 与 urllib 的依赖，绝不能排除）
        'tkinter', 'unittest', 'pydoc', 'doctest', 'test',
        'multiprocessing', 'concurrent', 'asyncio', 'sqlite3',
        'numpy', 'pandas', 'matplotlib', 'scipy', 'setuptools', 'pip',
    ],
    noarchive=False,
    optimize=2,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='AfterburnerWebMonitor',
    debug=False,
    bootloader_ignore_signals=False,
    strip=True,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    icon=['AfterburnerWebMonitor.ico'],
)
