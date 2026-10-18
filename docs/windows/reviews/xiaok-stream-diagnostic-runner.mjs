import {readFileSync,writeFileSync} from 'node:fs';
import {loadConfig} from '/Users/kk/.npm-global/lib/node_modules/xiaokcode/dist/utils/config.js';
import {createAdapter} from '/Users/kk/.npm-global/lib/node_modules/xiaokcode/dist/ai/models.js';
const base='/Users/kk/projects/beek/docs/windows/reviews/';
const config=await loadConfig();
async function probe(name,prompt){
 const adapter=createAdapter(config);const started=Date.now();
 const stats={case:name,model:adapter.getModelName(),input_bytes:Buffer.byteLength(prompt),deadline_ms:20000,request_count:0,packets:0,wire_text_chars:0,wire_reasoning_chars:0,adapter_text_chars:0,adapter_thinking_chars:0,chunk_types:{}};
 const hc=adapter.harnessContext;stats.profile=hc?.profile?.id;stats.provider=hc?.identity?.providerId;
 if(hc?.identity?.canonicalBaseUrl){stats.endpoint_host=new URL(hc.identity.canonicalBaseUrl).hostname;}
 const controller=new AbortController();const timer=setTimeout(()=>controller.abort(),20000);
 const owner=adapter.client.chat.completions;const original=owner.create.bind(owner);
 owner.create=async(...args)=>{
  stats.request_count++;stats.request_model=args[0]?.model;stats.request_reasoning_effort=args[0]?.reasoning_effort??null;
  stats.dispatch_ms=Date.now()-started;
  const pending=original(...args);let stream;
  if(typeof pending.withResponse==='function'){const result=await pending.withResponse();stats.http_status=result.response.status;stats.header_ms=Date.now()-started;stream=result.data;}
  else {stream=await pending;stats.header_ms=Date.now()-started;}
  const iter=stream[Symbol.asyncIterator].bind(stream);
  stream[Symbol.asyncIterator]=()=>{const it=iter();return {async next(){const n=await it.next();if(!n.done){stats.packets++;stats.first_packet_ms??=Date.now()-started;
    const choices=n.value?.choices??[];for(const c of choices){const d=c.delta??{};stats.wire_text_chars+=typeof d.content==='string'?d.content.length:0;
    for(const k of ['reasoning_content','reasoning','reasoning_text','thinking','thought']){if(typeof d[k]==='string')stats.wire_reasoning_chars+=d[k].length;}
    if(c.finish_reason!=null)stats.finish_reason=c.finish_reason;}
    stats.last_packet_ms=Date.now()-started;}
    return n;},return:it.return?.bind(it),throw:it.throw?.bind(it),[Symbol.asyncIterator](){return this;}};};return stream;
 };
 try{for await(const c of adapter.stream([{role:'user',content:[{type:'text',text:prompt}]}],[],'只做只读技术问答，直接回答，不调用工具。',{signal:controller.signal})){
 stats.chunk_types[c.type]=(stats.chunk_types[c.type]??0)+1;if(c.type==='text')stats.adapter_text_chars+=c.delta.length;if(c.type==='thinking')stats.adapter_thinking_chars+=c.delta.length;
 if(c.type==='done'){stats.done=true;break;}}
 }catch(e){stats.error_type=e?.name??'Error';stats.error_code=e?.code??null;stats.timed_out=controller.signal.aborted;}
 finally{clearTimeout(timer);adapter.dispose?.();stats.total_ms=Date.now()-started;}
 return stats;
}
const results=await Promise.all([probe('short','只回复 XIAOK_DIAG_READY。'),probe('section5',readFileSync(base+'round-part5-xiaok-native-prompt.txt','utf8'))]);
writeFileSync(base+'xiaok-stream-diagnostic.json',JSON.stringify({captured_at:new Date().toISOString(),results},null,2)+'\n');console.log(JSON.stringify(results));
