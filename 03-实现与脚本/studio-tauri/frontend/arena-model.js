export function worldToSvg(x,y){return {x:x*1000,y:3800-y*1000};}
export function sampleRates(previous,current){
  if(!previous||current.generation!==previous.generation)return {cpu:null,received:null,sent:null};
  const elapsed=current.clock-previous.clock,total=current.cpuTotal-previous.cpuTotal,idle=current.cpuIdle-previous.cpuIdle;
  const valid=elapsed>0&&elapsed<15;
  const cpu=valid&&total>0&&idle>=0&&idle<=total?100*(total-idle)/total:null;
  const before=new Map(previous.network.map(n=>[n.name,n]));let received=0,sent=0,count=0;
  for(const item of current.network){const old=before.get(item.name);if(old&&item.received>=old.received&&item.sent>=old.sent){received+=(item.received-old.received)/elapsed;sent+=(item.sent-old.sent)/elapsed;count++;}}
  return {cpu,received:valid&&count?received:null,sent:valid&&count?sent:null};
}
export function metricPath(points,key){
  if(!points.length)return '';const end=points.at(-1).clock;let connected=false,last=null;
  return points.map(sample=>{
    if(!Number.isFinite(sample[key])){connected=false;return '';}
    const command=connected&&sample.clock-last<8?'L':'M';connected=true;last=sample.clock;
    return `${command}${(Math.max(0,100-(end-sample.clock)/60*100)).toFixed(2)},${(100-Math.min(100,Math.max(0,sample[key]))).toFixed(2)}`;
  }).join(' ');
}
