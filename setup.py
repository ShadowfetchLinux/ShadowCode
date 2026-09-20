"""Include the compiled desktop in wheels; source development reads ui/dist."""
from pathlib import Path
import shutil
from setuptools import setup
from setuptools.command.build_py import build_py


class BuildWithDesktop(build_py):
    def run(self):
        root = Path(__file__).parent
        ui = root / 'ui' / 'dist'
        if not (ui / 'index.html').is_file():
            raise RuntimeError('Build the desktop first: npm --prefix ui ci && npm --prefix ui run build')
        super().run()
        assets = Path(self.build_lib) / 'shadow_agent' / '_assets'
        shutil.copytree(ui, assets / 'ui' / 'dist', dirs_exist_ok=True)
        (assets / 'assets' / 'icons').mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / 'assets' / 'icons' / 'shadow-agent.svg', assets / 'assets' / 'icons')


setup(cmdclass={'build_py': BuildWithDesktop})
