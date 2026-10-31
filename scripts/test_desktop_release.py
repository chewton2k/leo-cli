import base64
import json
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class ReleaseTests(unittest.TestCase):
    def test_updater_public_key_is_real_and_matches_configuration(self):
        key = (ROOT / 'crates/leo-desktop/updater-public-key.txt').read_text().strip()
        decoded = base64.b64decode(key, validate=True)
        self.assertIn(b'untrusted comment:', decoded)
        config = json.loads((ROOT / 'crates/leo-desktop/tauri.conf.json').read_text())
        self.assertEqual(config['plugins']['updater']['pubkey'], key)
        self.assertNotIn('version', config)

    def test_assets_and_manifest_match_all_three_targets(self):
        import tempfile
        import desktop_release
        with tempfile.TemporaryDirectory() as folder:
            folder = pathlib.Path(folder)
            for target, suffix in [('aarch64-apple-darwin', '.app.tar.gz'), ('x86_64-apple-darwin', '.app.tar.gz'), ('x86_64-pc-windows-msvc', '-setup.exe')]:
                name = 'leo-' + target + suffix
                (folder / name).write_bytes(b'archive')
                (folder / (name + '.sig')).write_text('signature-' + target)
            manifest = desktop_release.manifest(folder, '1.2.3', 'What changed', '2026-10-31T00:00:00Z')
            self.assertEqual(set(manifest['platforms']), {'darwin-aarch64', 'darwin-x86_64', 'windows-x86_64'})
            self.assertEqual(manifest['version'], '1.2.3')
            self.assertIn('/v1.2.3/', manifest['platforms']['darwin-aarch64']['url'])
            self.assertTrue(manifest['platforms']['windows-x86_64']['url'].endswith('-setup.exe'))
            self.assertEqual(manifest['platforms']['darwin-x86_64']['signature'], 'signature-x86_64-apple-darwin')
            (folder / 'leo-aarch64-apple-darwin.app.tar.gz.sig').unlink()
            with self.assertRaises(ValueError):
                desktop_release.manifest(folder, '1.2.3', '', '2026-10-31T00:00:00Z')

    def test_packaging_requires_exactly_one_signed_artifact(self):
        import tempfile
        import desktop_release
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            source, output = root / 'bundle', root / 'out'
            (source / 'macos').mkdir(parents=True)
            (source / 'dmg').mkdir()
            (source / 'macos/Leo.app.tar.gz').write_bytes(b'app')
            (source / 'dmg/Leo_1.2.3_aarch64.dmg').write_bytes(b'dmg')
            with self.assertRaises(ValueError):
                desktop_release.package(source, output, 'aarch64-apple-darwin')
            (source / 'macos/Leo.app.tar.gz.sig').write_text('signature')
            desktop_release.package(source, output, 'aarch64-apple-darwin')
            self.assertEqual((output / 'leo-aarch64-apple-darwin.app.tar.gz').read_bytes(), b'app')
            self.assertTrue((output / 'leo-aarch64-apple-darwin.dmg.sha256').is_file())
            (source / 'macos/Other.app.tar.gz').write_bytes(b'bad')
            with self.assertRaises(ValueError):
                desktop_release.package(source, output, 'aarch64-apple-darwin')

    def test_release_configuration_ships_cli_and_signed_updater_artifacts(self):
        config = json.loads((ROOT / 'crates/leo-desktop/tauri.conf.json').read_text())
        release = json.loads((ROOT / 'crates/leo-desktop/tauri.release.conf.json').read_text())
        self.assertTrue(config['bundle']['createUpdaterArtifacts'])
        self.assertEqual(release['bundle']['externalBin'], ['binaries/leo'])
        self.assertEqual(config['bundle']['windows']['nsis']['installMode'], 'currentUser')

    def test_workflow_builds_only_supported_signed_apps(self):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        build = workflow.split('  build:\n', 1)[1].split('  go-runner:', 1)[0]
        for target in ['aarch64-apple-darwin', 'x86_64-apple-darwin', 'x86_64-pc-windows-msvc']:
            self.assertIn(target, build)
        self.assertNotIn('linux', build)
        self.assertIn('tauri build --ci', build)
        self.assertIn('codesign --verify --deep --strict', build)
        self.assertIn('TAURI_SIGNING_PRIVATE_KEY_PASSWORD', build)
        self.assertIn('desktop_release.py manifest', workflow)
        self.assertIn('secrets: inherit', (ROOT / '.github/workflows/ci.yml').read_text())

    def test_linux_installer_exits_without_modifying_home(self):
        import tempfile
        import subprocess
        import os
        if os.name == 'nt':
            self.skipTest('The shell installer runs on macOS; Windows checks PowerShell instead.')
        with tempfile.TemporaryDirectory() as folder:
            folder = pathlib.Path(folder)
            fake = folder / 'tools'
            fake.mkdir()
            uname = fake / 'uname'
            uname.write_text('#!/bin/sh\ncase "$1" in -s) printf Linux;; *) printf x86_64;; esac\n')
            uname.chmod(0o755)
            home = folder / 'home'
            result = subprocess.run(['sh', str(ROOT / 'scripts/install-app.sh')], env={**os.environ, 'PATH':str(fake)+':/usr/bin:/bin','HOME':str(home)}, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('macOS and Windows only', result.stderr)
            self.assertFalse(home.exists())

    def test_installer_activation_requires_a_complete_published_app_release(self):
        import activate_app_installers
        import desktop_release
        manifest = {'version':'1.2.3', 'platforms':{}}
        names = {'latest.json', 'install.sh', 'install.ps1'}
        for target, (platform, suffix) in desktop_release.TARGETS.items():
            name = 'leo-' + target + suffix
            names.update([name, name+'.sig', name+'.sha256'])
            manifest['platforms'][platform] = {'signature':'signed','url':'https://github.com/chewton2k/leo-cli/releases/download/v1.2.3/'+name}
            if target.endswith('apple-darwin'):
                names.update(['leo-'+target+'.dmg', 'leo-'+target+'.dmg.sha256'])
        release = {'tag_name':'v1.2.3', 'draft':False, 'prerelease':False, 'assets':[{'name':name,'size':1} for name in names]}
        activate_app_installers.validate(release, manifest, '1.2.3')
        release['assets'] = [asset for asset in release['assets'] if not asset['name'].endswith('.app.tar.gz')]
        with self.assertRaises(ValueError):
            activate_app_installers.validate(release, manifest, '1.2.3')

    def test_activation_leaves_main_installers_untouched_before_release(self):
        import tempfile
        import subprocess
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            (root / 'install.sh').write_text('existing installer')
            dist = root / 'dist'
            dist.mkdir()
            (dist / 'install.sh').write_text('new app installer')
            (dist / 'install.ps1').write_text('new Windows installer')
            (dist / 'latest.json').write_text(json.dumps({'version':'1.2.3','platforms':{}}))
            release = root / 'release.json'
            release.write_text(json.dumps({'tag_name':'v1.2.3','assets':[]}))
            result = subprocess.run([__import__('sys').executable, str(ROOT / 'scripts/activate_app_installers.py'), str(release), str(dist), '1.2.3'], cwd=root, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual((root / 'install.sh').read_text(), 'existing installer')
            self.assertFalse((root / 'install.ps1').exists())


if __name__ == '__main__':
    unittest.main()
