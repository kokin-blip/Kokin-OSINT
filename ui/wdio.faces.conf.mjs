import {config as base} from './wdio.conf.mjs';
import {fileURLToPath} from 'node:url';
export const config={...base,specs:['./tests/desktop/faces.spec.mjs'],services:[[fileURLToPath(new URL('./tests/desktop/faces.service.mjs',import.meta.url)),{driverProvider:'embedded',captureBackendLogs:true,captureFrontendLogs:true}]]};
