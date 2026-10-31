import base64
import os
import pathlib
import secrets
import subprocess
import tempfile


def run(*args):
    result = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
    if result.returncode:
        raise SystemExit('The temporary macOS signing keychain could not be configured.')


def main():
    folder = pathlib.Path(tempfile.mkdtemp(prefix='leo-signing-', dir=os.environ['RUNNER_TEMP']))
    os.chmod(folder, 0o700)
    p12 = folder / 'identity.p12'
    p12.write_bytes(base64.b64decode(os.environ['APPLE_CERTIFICATE'], validate=True))
    os.chmod(p12, 0o600)
    keychain = str(folder / 'build.keychain-db')
    password = secrets.token_urlsafe(36)
    run('security', 'create-keychain', '-p', password, keychain)
    run('security', 'set-keychain-settings', '-lut', '21600', keychain)
    run('security', 'unlock-keychain', '-p', password, keychain)
    run('security', 'import', str(p12), '-k', keychain, '-P', os.environ['APPLE_CERTIFICATE_PASSWORD'], '-T', '/usr/bin/codesign', '-T', '/usr/bin/security')
    run('security', 'set-key-partition-list', '-S', 'apple-tool:,apple:,codesign:', '-s', '-k', password, keychain)
    cert = folder / 'identity.pem'
    exported = subprocess.run(['security', 'find-certificate', '-c', 'Leo Desktop Signing', '-p', keychain], capture_output=True, check=True, timeout=30)
    cert.write_bytes(exported.stdout)
    run('sudo', 'security', 'add-trusted-cert', '-d', '-r', 'trustRoot', '-p', 'codeSign', '-k', keychain, str(cert))
    run('security', 'list-keychains', '-d', 'user', '-s', keychain)
    run('security', 'default-keychain', '-d', 'user', '-s', keychain)
    p12.unlink()
    with open(os.environ['GITHUB_ENV'], 'a') as output:
        output.write('LEO_BUILD_KEYCHAIN=' + keychain + '\n')
        output.write('APPLE_SIGNING_IDENTITY=Leo Desktop Signing\n')
        output.write('LEO_APP_SIGNED=1\n')


if __name__ == '__main__':
    main()
