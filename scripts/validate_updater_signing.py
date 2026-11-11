import base64
import os
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
PUBLIC_KEY = ROOT / 'crates/leo-desktop/updater-public-key.txt'
MESSAGE = 'Updater signing failed. Replace TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD with the matching saved key and password, then rerun the release.'

MISMATCH = 'The updater signing key does not match crates/leo-desktop/updater-public-key.txt. Replace TAURI_SIGNING_PRIVATE_KEY with the saved key that belongs to it, then rerun the release.'


def key_id(encoded):
    found = base64.b64decode(base64.b64decode(encoded).decode().splitlines()[1])[2:10]
    if len(found) != 8:
        raise ValueError(MISMATCH)
    return found


def validate(env, runner=subprocess.run):
    if not all(env.get(name) for name in ('TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD')):
        raise ValueError(MESSAGE)
    with tempfile.TemporaryDirectory(prefix='leo-updater-check-') as folder:
        sample = pathlib.Path(folder) / 'probe'
        sample.write_text('Leo updater signing verification\n')
        command = ['pnpm', '--dir', str(ROOT / 'crates/leo-desktop'), 'tauri', 'signer', 'sign', str(sample)]
        try:
            result = runner(command, env=dict(env), capture_output=True, timeout=30)
        except (OSError, subprocess.TimeoutExpired):
            raise ValueError('The updater signing check could not finish. Check the desktop tool installation and retry.') from None
        signature = sample.with_suffix('.sig')
        if result.returncode or not signature.is_file() or not 0 < signature.stat().st_size <= 16384:
            raise ValueError(MESSAGE)
        try:
            matches = key_id(signature.read_text()) == key_id(PUBLIC_KEY.read_text())
        except (ValueError, IndexError):
            matches = False
        if not matches:
            raise ValueError(MISMATCH)


def main():
    try:
        validate(os.environ)
    except ValueError as error:
        raise SystemExit(str(error)) from None
    print('Updater signing key and password verified.')


if __name__ == '__main__':
    main()
