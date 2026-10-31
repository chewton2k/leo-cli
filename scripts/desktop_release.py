import argparse
import hashlib
import json
import pathlib
import re
import shutil
from datetime import datetime, timezone

TARGETS = {
    'aarch64-apple-darwin': ('darwin-aarch64', '.app.tar.gz'),
    'x86_64-apple-darwin': ('darwin-x86_64', '.app.tar.gz'),
    'x86_64-pc-windows-msvc': ('windows-x86_64', '-setup.exe'),
}


def only(source, pattern):
    found = list(source.glob(pattern))
    if len(found) != 1 or not found[0].is_file() or found[0].is_symlink():
        raise ValueError('Expected exactly one regular release artifact: ' + pattern)
    return found[0]


def checksum(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    path.with_name(path.name + '.sha256').write_text(digest.hexdigest() + '  ' + path.name + '\n')


def package(source, output, target):
    if target not in TARGETS:
        raise ValueError('Leo releases support macOS and Windows only.')
    output.mkdir(parents=True, exist_ok=True)
    _, suffix = TARGETS[target]
    pattern = 'nsis/*-setup.exe' if target.endswith('windows-msvc') else 'macos/*.app.tar.gz'
    archive = only(source, pattern)
    signature = archive.with_name(archive.name + '.sig')
    if not signature.is_file() or signature.is_symlink() or not 0 < signature.stat().st_size <= 8192:
        raise ValueError('A release requires an updater signature.')
    assets = [(archive, 'leo-' + target + suffix), (signature, 'leo-' + target + suffix + '.sig')]
    if target.endswith('apple-darwin'):
        assets.append((only(source, 'dmg/*.dmg'), 'leo-' + target + '.dmg'))
    for original, name in assets:
        dest = output / name
        shutil.copyfile(original, dest)
        checksum(dest)


def manifest(folder, version, notes, date):
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError('Release version must be X.Y.Z.')
    platforms = {}
    for target, (platform, suffix) in TARGETS.items():
        name = 'leo-' + target + suffix
        archive = folder / name
        signature = folder / (name + '.sig')
        if not archive.is_file() or archive.is_symlink() or not signature.is_file() or signature.is_symlink() or not 0 < signature.stat().st_size <= 8192:
            raise ValueError('Missing signed release asset: ' + name)
        platforms[platform] = {'signature': signature.read_text().strip(), 'url': 'https://github.com/chewton2k/leo-cli/releases/download/v' + version + '/' + name}
    return {'version': version, 'notes': notes[:8192], 'pub_date': date, 'platforms': platforms}


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest='command', required=True)
    pack = sub.add_parser('package')
    pack.add_argument('source', type=pathlib.Path)
    pack.add_argument('output', type=pathlib.Path)
    pack.add_argument('target', choices=TARGETS)
    latest = sub.add_parser('manifest')
    latest.add_argument('folder', type=pathlib.Path)
    latest.add_argument('version')
    latest.add_argument('notes', type=pathlib.Path)
    args = parser.parse_args()
    if args.command == 'package':
        package(args.source, args.output, args.target)
    else:
        notes = args.notes.read_text()[:8192]
        result = manifest(args.folder, args.version, notes, datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z'))
        (args.folder / 'latest.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
