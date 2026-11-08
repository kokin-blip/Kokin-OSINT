import {config as base} from './wdio.conf.mjs';
import {fileURLToPath} from 'node:url';
export const config={...base,specs:['./tests/desktop/research.spec.mjs'],services:[[fileURLToPath(new URL('./tests/desktop/research.service.mjs',import.meta.url)),{driverProvider:'embedded',captureBackendLogs:true,captureFrontendLogs:true}]]};
