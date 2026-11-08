// Tauri service 1.5 reads the removed WDIO 10 executeAsync alias during setup.
// Its embedded provider uses the awaited execute endpoint; restore the alias
// only in this acceptance harness, without patching installed packages.
import TauriService from '@wdio/tauri-service';
export {launcher} from '@wdio/tauri-service';
export default class FacesDesktopService extends TauriService {
 async before(capabilities,specs,browser){
  if(typeof browser.executeAsync!=='function')browser.addCommand('executeAsync',function(script,...args){return this.execute(script,...args);});
  // The native 1.3 plugin predates the 1.5 service's preserved-core global.
  // Bootstrap it through plain WebDriver before the service's direct-eval path.
  await browser.executeScript("if(!window.__wdio_original_core__){const core=window.__TAURI__.core;window.__wdio_original_core__={invoke:core.invoke.bind(core)};}return true;",[]);
  return super.before(capabilities,specs,browser);
 }
}
