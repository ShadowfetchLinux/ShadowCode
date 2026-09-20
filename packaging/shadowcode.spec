from pathlib import Path
from PyInstaller.utils.hooks import collect_submodules, copy_metadata
root = Path(SPECPATH).parent
metadata = []
for package in ['shadow-agent', 'mcp', 'typer', 'rich', 'uvicorn', 'pydantic', 'fastapi']:
    metadata += copy_metadata(package)
a = Analysis([str(root / 'packaging' / 'entry.py')], pathex=[str(root / 'src')],
    binaries=[], datas=[(str(root / 'ui' / 'dist'), 'ui/dist'), (str(root / 'assets' / 'icons' / 'shadow-agent.svg'), 'assets/icons')] + metadata,
    hiddenimports=collect_submodules('shadow_agent') + ['uvicorn.logging', 'uvicorn.loops.auto', 'uvicorn.protocols.http.auto', 'uvicorn.protocols.websockets.auto', 'uvicorn.lifespan.on'],
    excludes=['pytest', 'tkinter'], noarchive=False)
pyz = PYZ(a.pure)
exe = EXE(pyz, a.scripts, [], exclude_binaries=True, name='shadowcode', debug=False, strip=False, upx=False, console=True)
coll = COLLECT(exe, a.binaries, a.datas, strip=False, upx=False, name='ShadowCode')
