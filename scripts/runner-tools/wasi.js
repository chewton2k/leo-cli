import { WASI, File, OpenFile, ConsoleStdout, PreopenDirectory, Directory } from '@bjorn3/browser_wasi_shim';

self.leoWasi = { WASI, File, OpenFile, ConsoleStdout, PreopenDirectory, Directory };
