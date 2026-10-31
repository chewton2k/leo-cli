import json
import pathlib
import shutil
import sys

from desktop_release import TARGETS


def validate(release, manifest, version):
    if release.get('tag_name') != 'v' + version or release.get('draft') or release.get('prerelease'):
        raise ValueError('Only the latest published app release can activate installers.')
    required = {'latest.json', 'install.sh', 'install.ps1'}
    platforms = manifest.get('platforms', {})
    if manifest.get('version') != version or set(platforms) != {item[0] for item in TARGETS.values()}:
        raise ValueError('The updater manifest does not cover this release.')
    for target, (platform, suffix) in TARGETS.items():
        name = 'leo-' + target + suffix
        required.update([name, name + '.sig', name + '.sha256'])
        if target.endswith('apple-darwin'):
            required.update(['leo-' + target + '.dmg', 'leo-' + target + '.dmg.sha256'])
        entry = platforms[platform]
        if not entry.get('signature') or entry.get('url') != 'https://github.com/chewton2k/leo-cli/releases/download/v' + version + '/' + name:
            raise ValueError('An updater platform is missing its signature or release URL.')
    assets = {item['name'] for item in release.get('assets', []) if item.get('size', 0) > 0}
    if not required <= assets:
        raise ValueError('Keep existing installers until every app asset has been published.')


def main():
    release, folder, version = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3]
    validate(json.loads(release.read_text()), json.loads((folder / 'latest.json').read_text()), version)
    for name in ['install.sh', 'install.ps1']:
        dest = pathlib.Path(name)
        if dest.is_symlink():
            raise ValueError('An installer entry point must be a regular file.')
        shutil.copyfile(folder / name, dest)
    pathlib.Path('install.sh').chmod(0o755)


if __name__ == '__main__':
    main()
