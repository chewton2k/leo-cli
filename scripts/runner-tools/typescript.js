import { transform } from 'sucrase';

self.leoTypeScript = (code) => transform(code, { transforms: ['typescript'], disableESTransforms: true }).code;
