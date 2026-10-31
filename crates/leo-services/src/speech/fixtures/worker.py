import json
import os
import pathlib
import struct
import sys
import time

cpu = '--cpu-only' in sys.argv
mode = ''
folder = None

def read_exact(length):
    out = b''
    while len(out) < length:
        part = sys.stdin.buffer.read(length - len(out))
        if not part:
            raise EOFError()
        out += part
    return out

def send(value):
    payload = json.dumps(value).encode()
    sys.stdout.buffer.write(struct.pack('<I', len(payload)) + payload + struct.pack('<I', 0))
    sys.stdout.buffer.flush()

try:
    while True:
        request = json.loads(read_exact(struct.unpack('<I', read_exact(4))[0]))
        read_exact(struct.unpack('<I', read_exact(4))[0])
        if 'Hello' in request:
            send({'Hello': {'devices': None}})
        elif 'Load' in request:
            path = pathlib.Path(request['Load']['path'])
            mode = path.name
            folder = path.parent
            with (folder / 'pids').open('a') as file:
                file.write(str(os.getpid()) + '\n')
            send({'Loaded': {'arch': 'fake', 'variant': 'fake', 'backend': 'cpu' if cpu else 'metal', 'device': 'fake', 'on_gpu': not cpu,
                'supports_initial_prompt': False, 'supports_long_form': False, 'supports_vocabulary': False,
                'capabilities': {'native_sample_rate': 16000, 'languages': ['en'], 'translate_target_languages': [],
                'max_timestamp_kind': 'None', 'supports_language_detect': False, 'supports_translate': False,
                'supports_streaming': False, 'supports_spec_decode': False, 'max_audio_ms': 0}}})
        elif 'Run' in request:
            (folder / 'running').touch()
            if mode == 'hung':
                time.sleep(30)
            if not cpu and mode == 'crashed':
                os._exit(7)
            if not cpu and mode == 'garbage':
                sys.stdout.buffer.write(struct.pack('<I', 0xffffffff))
                sys.stdout.buffer.flush()
                os._exit(8)
            send({'Transcript': {'text': 'local words'}})
        else:
            send('Ok')
except EOFError:
    pass
