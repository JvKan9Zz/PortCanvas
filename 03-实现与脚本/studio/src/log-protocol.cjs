const {EventEmitter}=require('node:events');
const {StringDecoder}=require('node:string_decoder');

function decodeOutput(text) {
  const chunks=[];let start=0;
  for(const match of text.matchAll(/\\([0-7]{3})/g)) {
    chunks.push(Buffer.from(text.slice(start,match.index),'utf8'),Buffer.from([parseInt(match[1],8)]));start=match.index+4;
  }
  chunks.push(Buffer.from(text.slice(start),'utf8'));return Buffer.concat(chunks);
}
function parsePacket(packet) {
  if(typeof packet!=='string'||packet.length>150000)return null;
  const p=packet.split(';');
  if(p.length!==9||!['ready','begin','end'].includes(p[0])||!/^\d+-\d+$/.test(p[1])||!/^\d{1,12}$/.test(p[2]))return null;
  if(p[0]==='end'&&!/^\d{1,3}$/.test(p[3]))return null;
  const decode=value=>{if(!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value))throw Error('Invalid base64');return Buffer.from(value,'base64').toString('utf8');};
  try{return {kind:p[0],shell:p[1],sequence:p[2],code:p[0]==='end'?Number(p[3]):null,command:decode(p[4]),cwd:decode(p[5]),projectRoot:decode(p[6]),endCwd:decode(p[7]),backgroundJobs:p[8].split(',').filter(v=>/^\d+$/.test(v))};}catch{return null;}
}

// The control client supplies raw pane output, before tmux redraws the screen.
// Keep command boundaries separate from text and normalize terminal line edits.
class OutputParser extends EventEmitter {
  constructor(){super();this.decoder=new StringDecoder('utf8');this.mode='text';this.line='';this.osc='';this.cr=false;}
  push(data){this.consume(this.decoder.write(data));}
  flush(){this.consume(this.decoder.end());this.flushLine();}
  flushLine(newline=false){if(this.line||newline)this.emit('text',this.line+(newline?'\n':''));this.line='';this.cr=false;}
  consume(text){
    for(const ch of text){
      if(this.mode==='osc'||this.mode==='osc-escape'){
        if(ch==='\x07'||(this.mode==='osc-escape'&&ch==='\\')){
          if(this.osc.startsWith('777;WCSLOG;')){const event=parsePacket(this.osc.slice(11));if(event){this.flushLine();this.emit('marker',event);}}
          this.osc='';this.mode='text';continue;
        }
        if(ch==='\x1b'){this.mode='osc-escape';continue;}
        if(this.osc.length<150032)this.osc+=ch;else{this.osc='';this.mode='discard';}
        if(this.mode!=='discard')this.mode='osc';continue;
      }
      if(this.mode==='discard'){if(ch==='\x07')this.mode='text';else if(ch==='\x1b')this.mode='discard-escape';continue;}
      if(this.mode==='discard-escape'){this.mode=ch==='\\'?'text':'discard';continue;}
      if(this.mode==='csi'){if(/[\x40-\x7e]/.test(ch))this.mode='text';continue;}
      if(this.mode==='charset'){this.mode='text';continue;}
      if(this.mode==='escape'){
        this.mode=ch==='['?'csi':ch===']'?'osc':['P','^','_'].includes(ch)?'discard':['(',')','*','+'].includes(ch)?'charset':'text';continue;
      }
      if(ch==='\x1b'){this.mode='escape';continue;}
      if(ch==='\r'){this.cr=true;continue;}
      if(ch==='\n'){this.flushLine(true);continue;}
      if(this.cr)this.flushLine(true);
      if(ch==='\b'){this.line=Array.from(this.line).slice(0,-1).join('');continue;}
      if(ch==='\t'||ch>=' '){this.line+=ch;if(this.line.length>=65536)this.flushLine();}
    }
  }
}
module.exports={decodeOutput,parsePacket,OutputParser};
