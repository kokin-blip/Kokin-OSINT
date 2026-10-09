import {resolve} from 'node:path';
export const config={
  runner:'local',specs:['./tests/desktop/**/*.spec.mjs'],maxInstances:1,
  capabilities:[{browserName:'tauri','tauri:options':{application:resolve(process.platform==='darwin'?'../target/debug/bundle/macos/Kokin-OSINT.app/Contents/MacOS/kokin-osint':'../target/debug/kokin-osint.exe')}}],
  services:[['@wdio/tauri-service',{driverProvider:'embedded',captureBackendLogs:true,captureFrontendLogs:true}]],
  framework:'mocha',reporters:['spec'],mochaOpts:{timeout:180000},waitforTimeout:30000,
};
