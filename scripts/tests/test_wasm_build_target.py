import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class WasmBuildTargetTests(unittest.TestCase):
    def test_worktrees_have_stable_distinct_caches(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binaries = root / 'bin'
            binaries.mkdir()
            for name, source in {
                'rustup': '#!/bin/sh\nexit 0\n',
                'wasm-pack': '#!/bin/sh\nprintf "%s\\n" "$CARGO_TARGET_DIR"\n',
            }.items():
                path = binaries / name
                path.write_text(source)
                path.chmod(0o755)
            cache = root / 'shared'
            env = dict(os.environ, PATH=f'{binaries}{os.pathsep}{os.environ["PATH"]}',
                       CARGO_TARGET_DIR=str(cache))
            source = Path(__file__).resolve().parents[1] / 'build_wasm_runtime.sh'

            def run(name, settings):
                wrapper = root / name / 'scripts' / source.name
                wrapper.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, wrapper)
                return Path(subprocess.check_output(
                    ['bash', wrapper, '--target', 'web', '--dev'],
                    text=True, env=settings).strip())

            first = run('one', env)
            self.assertEqual(run('one', env), first)
            self.assertNotEqual(run('two', env), first)
            self.assertEqual(first.parent, cache / 'rport-checkouts')
            default = dict(env)
            del default['CARGO_TARGET_DIR']
            self.assertEqual(run('one', default), root / 'one/target/wasm-unwind')


if __name__ == '__main__':
    unittest.main()
