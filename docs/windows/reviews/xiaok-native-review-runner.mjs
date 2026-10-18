import {readFileSync,writeFileSync} from 'node:fs';
import {loadConfig} from '/Users/kk/.npm-global/lib/node_modules/xiaokcode/dist/utils/config.js';
import {createAdapter} from '/Users/kk/.npm-global/lib/node_modules/xiaokcode/dist/ai/models.js';
import {createAdapterBoundaryInvoker} from '/Users/kk/.npm-global/lib/node_modules/xiaokcode/dist/ai/intent-delegation/llm-boundary-classifier.js';
const base='/Users/kk/projects/beek/docs/windows/reviews/';
const round=process.argv[2]||'1';
const prompt=readFileSync(base+`round-${round}-xiaok-native-prompt.txt`,'utf8');
const config=await loadConfig(); const adapter=createAdapter(config);
const meta={reviewer:'xiaok',mode:'installed-native-adapter',model:adapter.getModelName(),round:Number(round),started_at:new Date().toISOString()};
const controller=new AbortController(); const timer=setTimeout(()=>controller.abort(),120000);
try {const invoker=createAdapterBoundaryInvoker(adapter,{timeoutMs:120000});
 const result=await invoker.invoke(prompt,controller.signal);
 writeFileSync(base+`round-${round}-xiaok-native-response.txt`,result+'\n');
 meta.exit_code=0;meta.completed_at=new Date().toISOString();
} catch(e) {meta.exit_code=1;meta.error_type=e?.name||'Error';meta.timed_out=controller.signal.aborted;}
finally {clearTimeout(timer);adapter.dispose?.();writeFileSync(base+`round-${round}-xiaok-native-run.json`,JSON.stringify(meta,null,2)+'\n');console.log(JSON.stringify(meta));}
