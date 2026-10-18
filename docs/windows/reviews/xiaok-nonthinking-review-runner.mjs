import {readFileSync,writeFileSync,appendFileSync} from 'node:fs';
import {loadConfig} from '/Users/kk/.npm-global/lib/node_modules/xiaokcode/dist/utils/config.js';
import {createAdapter} from '/Users/kk/.npm-global/lib/node_modules/xiaokcode/dist/ai/models.js';
const base='/Users/kk/projects/beek/docs/windows/reviews/';
const round=process.argv[2]||'2';const prompt=readFileSync(base+`round-${round}-xiaok-stream-prompt.txt`,'utf8');
const adapter=createAdapter(await loadConfig());const owner=adapter.client.chat.completions;const originalCreate=owner.create.bind(owner);owner.create=(body,opts)=>originalCreate({...body,enable_thinking:false},opts);const start=Date.now();let lastActivity=start;let text='';
const meta={reviewer:'xiaok',model:adapter.getModelName(),mode:'native-stream-with-progress',round:Number(round),started_at:new Date().toISOString(),idle_limit_ms:60000,total_limit_ms:900000,max_output_tokens:16384,enable_thinking_requested:false,thinking_chars:0,text_chars:0,chunks:0,chunk_types:{}};
const ctrl=new AbortController();
const progress=()=>{const event={elapsed_s:Math.round((Date.now()-start)/1000),thinking_chars:meta.thinking_chars,text_chars:meta.text_chars,chunks:meta.chunks};appendFileSync(base+`round-${round}-xiaok-progress.jsonl`,JSON.stringify(event)+'\n');console.log(JSON.stringify(event));};
const timer=setInterval(()=>{if(Date.now()-lastActivity>60000){meta.stop_reason='idle_timeout';ctrl.abort();}if(Date.now()-start>45000 && meta.thinking_chars>256 && meta.text_chars===0){meta.stop_reason='nonthinking_mode_not_honored';ctrl.abort();}if(Date.now()-start>900000){meta.stop_reason='total_timeout';ctrl.abort();}},1000);
const heartbeat=setInterval(progress,15000);
try {for await(const c of adapter.stream([{role:'user',content:[{type:'text',text:prompt}]}],[],'独立技术设计评审，只输出最终JSON意见，不调用工具。',{signal:ctrl.signal,maxOutputTokens:16384})){
 lastActivity=Date.now();meta.first_chunk_ms??=Date.now()-start;meta.chunks++;meta.chunk_types[c.type]=(meta.chunk_types[c.type]??0)+1;
 if(c.type==='thinking')meta.thinking_chars+=c.delta.length;
 if(c.type==='text'){meta.text_chars+=c.delta.length;text+=c.delta;if(Buffer.byteLength(text)>262144){meta.stop_reason='output_size_limit';ctrl.abort();}}
 if(c.type==='done'){meta.done=true;break;}}
 meta.exit_code=meta.done?0:1;
} catch(e){meta.exit_code=1;meta.error_type=e?.name??'Error';meta.error_code=e?.code??null;meta.http_error_status=e?.status??null;meta.timed_out=ctrl.signal.aborted;}
finally{clearInterval(timer);clearInterval(heartbeat);adapter.dispose?.();meta.elapsed_s=Math.round((Date.now()-start)/1000);progress();writeFileSync(base+`round-${round}-xiaok-stream-response.txt`,text+'\n');writeFileSync(base+`round-${round}-xiaok-stream-run.json`,JSON.stringify(meta,null,2)+'\n');console.log(JSON.stringify(meta));}
