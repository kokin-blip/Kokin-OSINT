// Adapt the installed service's element override API to WebdriverIO 10.
// The compatibility wrapper is test-only and never enters the application.
import BaseService from './faces.service.mjs';
export {launcher} from './faces.service.mjs';
export default class ResearchDesktopService extends BaseService {
 async before(capabilities,specs,browser){
  const overwrite=browser.overwriteCommand.bind(browser);
  browser.overwriteCommand=(name,fn,options)=>overwrite(name,fn,typeof options==='boolean'?{attachToElement:options}:options);
  return super.before(capabilities,specs,browser);
 }
}
